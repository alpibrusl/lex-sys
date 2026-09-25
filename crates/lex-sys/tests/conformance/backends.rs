//! `--backend cranelift|llvm` (`docs/llvm-backend.md` §4): the CLI wiring
//! for the second backend's first slice, checked the way
//! `differential.rs` checks the constant folder -- two independent paths
//! agreeing on one answer is the point, not a detail of either.

use super::*;

fn build_with(tag: &str, relative: &str, backend: &str) -> std::process::Output {
    let dir = scratch(tag);
    let exe = dir.join("out");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            repo_root().join(relative).as_os_str(),
            "--std".as_ref(),
            "--backend".as_ref(),
            backend.as_ref(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    if !build.status.success() {
        let _ = std::fs::remove_dir_all(&dir);
        return build;
    }
    // Neither fixture this module builds declares `//~ STDIN`, so a plain
    // run -- no piped input -- is exact, not merely convenient.
    let run = Command::new(&exe).output().expect("the compiled program runs");
    let _ = std::fs::remove_dir_all(&dir);
    run
}

fn assert_backends_agree(tag: &str, relative: &str, expected_stdout: &str) {
    let cranelift = build_with(&format!("{tag}-cranelift"), relative, "cranelift");
    let llvm = build_with(&format!("{tag}-llvm"), relative, "llvm");

    assert!(
        llvm.status.success(),
        "`--backend llvm` should build and run `{relative}`, but said:\n{}",
        String::from_utf8_lossy(&llvm.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&llvm.stdout),
        String::from_utf8_lossy(&cranelift.stdout),
        "the two backends printed different things for the same program"
    );
    assert_eq!(
        llvm.status.code(),
        cranelift.status.code(),
        "the two backends exited differently for the same program"
    );
    assert_eq!(String::from_utf8_lossy(&llvm.stdout), expected_stdout);
    assert_eq!(llvm.status.code(), Some(0));
}

/// The LLVM backend's first slice (`docs/llvm-backend.md` §5) builds
/// `tests/accept/llvm_smoke.ls` -- the fixture the doc's original bullet
/// list actually describes -- and the two backends agree with each other
/// and with the fixture's own `//~ STDOUT`/`//~ EXIT` directives.
#[test]
fn the_two_backends_agree_on_the_llvm_smoke_fixture() {
    assert_backends_agree("backends-smoke", "tests/accept/llvm_smoke.ls", "Hi!\n");
}

/// §5's second slice: checked arithmetic. `tests/accept/llvm_arith.ls`
/// exercises every trapping `BinOp` plus the bitwise operators, and the
/// two backends compute byte-for-byte the same answers.
#[test]
fn the_two_backends_agree_on_the_llvm_arith_fixture() {
    assert_backends_agree("backends-arith", "tests/accept/llvm_arith.ls", "Hi! OK$iK\n");
}

/// §5's third slice: control flow. `tests/accept/llvm_control.ls`
/// exercises `if`/`else`, `while`, every comparison and both
/// short-circuit operators, and the two backends compute byte-for-byte
/// the same answers.
#[test]
fn the_two_backends_agree_on_the_llvm_control_fixture() {
    assert_backends_agree(
        "backends-control",
        "tests/accept/llvm_control.ls",
        "01X34Z\n+-42\nF\n!T\nT\n!T\n",
    );
}

/// §5's fourth slice: slices and strings. `examples/hello.ls` -- the
/// program §5 originally (and wrongly) named as the first slice's own
/// target -- is what closes the loop: `ci.yml`'s own smoke test, built
/// and run through the second backend, byte for byte the same as the
/// first.
#[test]
fn the_two_backends_agree_on_hello_ls() {
    assert_backends_agree("backends-hello", "examples/hello.ls", "Hello, world!\n");
}

/// §5's fifth slice: structs and enums. `tests/accept/enums.ls`
/// exercises a struct literal, an enum with payloads (one of them
/// struct-typed), and `match` -- a chain of tag tests, a wildcard arm,
/// and field access on an owned matched binding -- and the two backends
/// compute byte-for-byte the same answers.
#[test]
fn the_two_backends_agree_on_the_enums_fixture() {
    assert_backends_agree("backends-enums", "tests/accept/enums.ls", "001220069901\n");
}

/// The boundary this slice draws is a located refusal, not a crash or a
/// silent wrong answer: `region`/`alloc_slice` need the arena allocation
/// this backend does not lower yet (`docs/llvm-backend.md` §5).
#[test]
fn a_program_outside_this_backend_is_refused_through_the_cli() {
    let dir = scratch("backends-llvm-arena");
    let exe = dir.join("out");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            repo_root().join("tests/accept/arena_roundtrip.ls").as_os_str(),
            "--std".as_ref(),
            "--backend".as_ref(),
            "llvm".as_ref(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        !build.status.success(),
        "`arena_roundtrip.ls` is outside this backend and should refuse"
    );
    assert_eq!(build.status.code(), Some(1), "an unsupported program is rule `internal`, exit 1");
    let message = String::from_utf8_lossy(&build.stderr).to_lowercase();
    assert!(message.contains("region"), "the refusal should name the boundary it hit: {message}");
}
