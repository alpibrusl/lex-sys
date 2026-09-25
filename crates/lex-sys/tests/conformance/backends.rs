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

/// `docs/llvm-backend.md` §7: three of `benches/`' loop-heavy kernels
/// build on both backends already -- `sum_checked.ls` (tight checked
/// arithmetic, no memory traffic), `fib_checked.ls` (recursion, so the
/// cost is calls rather than arithmetic) and `benches/three/mandelbrot.ls`
/// (Q16.16 fixed-point compute, `docs/against-c-and-rust.md`'s own
/// kernel). Both communicate correctness through their exit code
/// (`result - expected`, zero when right) rather than `stdout`, except
/// `mandelbrot.ls`, which prints a checksum -- `scripts/backend_compare.py`
/// is where the timing comes from; this is only agreement.
#[test]
fn the_two_backends_agree_on_sum_checked() {
    assert_backends_agree("backends-sum", "benches/sum_checked.ls", "");
}

#[test]
fn the_two_backends_agree_on_fib_checked() {
    assert_backends_agree("backends-fib", "benches/fib_checked.ls", "");
}

#[test]
fn the_two_backends_agree_on_mandelbrot() {
    assert_backends_agree("backends-mandelbrot", "benches/three/mandelbrot.ls", "39690297\n");
}

/// `docs/llvm-backend.md` §7.3's first-named gap, closed: `wrapping_add`/
/// `sub`/`mul` are LLVM's own `add`/`sub`/`mul`, already two's-complement
/// wraparound with no `nsw`/`nuw` requested, so unlike `binop`'s checked
/// forms this needs no overflow check at all. Closing it makes every
/// checked-vs-wrapping pair in `benches/` buildable on `--backend llvm`
/// for the first time, and unblocks `benches/three/purity.ls` besides.
#[test]
fn the_two_backends_agree_on_sum_wrapping() {
    assert_backends_agree("backends-sum-wrapping", "benches/sum_wrapping.ls", "");
}

#[test]
fn the_two_backends_agree_on_fib_wrapping() {
    assert_backends_agree("backends-fib-wrapping", "benches/fib_wrapping.ls", "");
}

#[test]
fn the_two_backends_agree_on_purity() {
    assert_backends_agree("backends-purity", "benches/three/purity.ls", "-7463529374017724416\n");
}

/// `docs/llvm-backend.md` §7.5: `region`/`alloc_slice` closed -- one
/// `malloc` in, one `free` out, and a bump pointer kept in two `ptr`-typed
/// `alloca` cells rather than in an SSA `Variable`, the same arena
/// `lex-sys-codegen`'s own `body/memory.rs` already builds. `byte_of` and
/// `!` (`Expr::Not`) closed alongside it -- both were the only things
/// standing between this and `sieve`/`scan` actually building.
#[test]
fn the_two_backends_agree_on_sieve_checked() {
    assert_backends_agree("backends-sieve-checked", "benches/sieve_checked.ls", "");
}

#[test]
fn the_two_backends_agree_on_sieve_wrapping() {
    assert_backends_agree("backends-sieve-wrapping", "benches/sieve_wrapping.ls", "");
}

#[test]
fn the_two_backends_agree_on_scan_checked() {
    assert_backends_agree("backends-scan-checked", "benches/scan_checked.ls", "");
}

#[test]
fn the_two_backends_agree_on_scan_wrapping() {
    assert_backends_agree("backends-scan-wrapping", "benches/scan_wrapping.ls", "");
}

#[test]
fn the_two_backends_agree_on_the_three_language_sieve() {
    assert_backends_agree("backends-sieve-three", "benches/three/sieve.ls", "6057\n");
}

/// `docs/llvm-backend.md` §7.7: heap boxing (`box_slice`/`contents`/
/// `unbox_slice`) closed, plus multi-leaf returns -- `fill` in
/// `reduce_checked.ls` returns `Box[[int]]`, two leaves, which the LLVM
/// backend could not hand back out of a call at all until this slice.
#[test]
fn the_two_backends_agree_on_reduce_checked() {
    assert_backends_agree("backends-reduce-checked", "benches/reduce_checked.ls", "");
}

#[test]
fn the_two_backends_agree_on_reduce_wrapping() {
    assert_backends_agree("backends-reduce-wrapping", "benches/reduce_wrapping.ls", "");
}

#[test]
fn the_two_backends_agree_on_layout_aos() {
    assert_backends_agree("backends-layout-aos", "benches/layout/aos.ls", "32000000\n");
}

#[test]
fn the_two_backends_agree_on_layout_soa() {
    assert_backends_agree("backends-layout-soa", "benches/layout/soa.ls", "32000000\n");
}

#[test]
fn the_two_backends_agree_on_layout_ints() {
    assert_backends_agree("backends-layout-ints", "benches/layout/ints.ls", "192000000\n");
}

#[test]
fn the_two_backends_agree_on_layout_rgb() {
    assert_backends_agree("backends-layout-rgb", "benches/layout/rgb.ls", "192000000\n");
}

/// `docs/llvm-backend.md` §7.9: `getchar` closed (the mirror of
/// `putchar`, sign-extended the same way), plus `s[a..b]`
/// (`Expr::Subslice`) found sitting in front of `revcomp.ls` once it was
/// tried against it. `tests/accept/stdin_roundtrip.ls` is the first
/// fixture here needing piped input, so it gets its own comparison
/// rather than reusing `assert_backends_agree`, which never pipes one.
#[test]
fn the_two_backends_agree_on_stdin_roundtrip() {
    let stdin = "hello\nworld\n";
    let expected = "hello\nworld\n12 bytes\n";
    for backend in ["cranelift", "llvm"] {
        let dir = scratch(&format!("backends-stdin-{backend}"));
        let exe = dir.join("out");
        let build = Command::new(BIN)
            .args([
                "build".as_ref(),
                repo_root().join("tests/accept/stdin_roundtrip.ls").as_os_str(),
                "--std".as_ref(),
                "--backend".as_ref(),
                backend.as_ref(),
                "-o".as_ref(),
                exe.as_os_str(),
            ])
            .output()
            .expect("the compiler runs");
        assert!(
            build.status.success(),
            "`--backend {backend}` should build `stdin_roundtrip.ls`, but said:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );

        let mut child = Command::new(&exe)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("the program runs");
        child
            .stdin
            .take()
            .expect("a piped stdin")
            .write_all(stdin.as_bytes())
            .expect("the program accepts its input");
        let output = child.wait_with_output().expect("the program finishes");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(output.status.code(), Some(0), "`--backend {backend}` should exit 0");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "`--backend {backend}` printed the wrong thing"
        );
    }
}

/// `docs/llvm-backend.md` §7.11: `Place::Field`/`Place::Deref` closed --
/// writing through a reference, whole-referent or one field, plus the
/// read-side siblings (`Expr::FieldRef`/`Expr::FieldAddr`/`Expr::Deref`)
/// nothing had built either. `tests/accept/deref_roundtrip.ls` already
/// existed as exactly this fixture: `*n`/`*n = e` (a shared and a unique
/// reference to a bare `int`), and `scale`'s `p.x`/`p.y` reading a field
/// *through* a reference before `*p = Point { .. }` replaces the whole
/// referent.
#[test]
fn the_two_backends_agree_on_deref_roundtrip() {
    assert_backends_agree(
        "backends-deref-roundtrip",
        "tests/accept/deref_roundtrip.ls",
        "41 42\n3 4 -> 30 40\n",
    );
}

/// §7.11 closed tuples alongside struct/reference writes: `Expr::Tuple`,
/// `Expr::TupleField` (an owner's `.0`/`.1`) and `Expr::TupleFieldRef`
/// (the same, through a reference -- `sum`'s own `pair.0 + pair.1`
/// where `pair: &p (int, int)`), none of them built before this slice
/// either. `tests/accept/tuple_roundtrip.ls` already existed as exactly
/// this fixture.
#[test]
fn the_two_backends_agree_on_tuple_roundtrip() {
    assert_backends_agree(
        "backends-tuple-roundtrip",
        "tests/accept/tuple_roundtrip.ls",
        "7 true\n42\n",
    );
}

/// §7.11 also closed `write_bytes`/`write_err` (`docs/bulk-io.md` §3),
/// found trying to build `revcomp.ls` once field/reference writes no
/// longer stood in front of it -- `fwrite` through `stdout`/`stderr`,
/// the platform-specific symbol `lex-sys-codegen`'s own `emit.rs`
/// already resolves. `revcomp.ls` is the real target #106 traced this
/// boundary to, checked against the Benchmarks Game's own reference
/// output the same way `benchmarks.rs`'s
/// `fasta_and_reverse_complement_print_the_published_answer` already
/// does for `--backend cranelift`.
#[test]
fn the_two_backends_agree_on_revcomp() {
    let root = repo_root().join("benches").join("game");
    let stdin = std::fs::read_to_string(root.join("fasta-1000.txt")).expect("fasta-1000.txt");
    let expected =
        std::fs::read_to_string(root.join("revcomp-1000.txt")).expect("revcomp-1000.txt");

    for backend in ["cranelift", "llvm"] {
        let dir = scratch(&format!("backends-revcomp-{backend}"));
        let exe = dir.join("out");
        let build = Command::new(BIN)
            .args([
                "build".as_ref(),
                root.join("revcomp.ls").as_os_str(),
                "--std".as_ref(),
                "--backend".as_ref(),
                backend.as_ref(),
                "-o".as_ref(),
                exe.as_os_str(),
            ])
            .output()
            .expect("the compiler runs");
        assert!(
            build.status.success(),
            "`--backend {backend}` should build `revcomp.ls`, but said:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );

        let mut child = Command::new(&exe)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("the program runs");
        child
            .stdin
            .take()
            .expect("a piped stdin")
            .write_all(stdin.as_bytes())
            .expect("the program accepts its input");
        let output = child.wait_with_output().expect("the program finishes");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(output.status.code(), Some(0), "`--backend {backend}` should exit 0");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "`--backend {backend}` should print what the Benchmarks Game publishes"
        );
    }
}

/// `docs/llvm-backend.md` §7.13: `arg_count`/`arg` closed, `argc`/`argv`
/// stashed once into module-local storage by `@main` before the entry
/// function's own body runs (`docs/arguments.md` §3). No arguments are
/// passed here -- `assert_backends_agree` never does -- so this only
/// exercises the "one argument, its own name" path; `arg(a, 0)` and the
/// bounds trap are `arg_count_and_arg_build_and_run_the_arguments_fixture`'s
/// job in the crate's own suite, and real arguments are what
/// `the_two_backends_agree_on_fannkuch`, below, passes.
#[test]
fn the_two_backends_agree_on_arguments() {
    assert_backends_agree("backends-arguments", "tests/accept/arguments.ls", "1\nnamed: 1\n");
}

/// `fannkuch.ls` itself, past `arg_count`/`arg`: the boundary #106 traced
/// for `revcomp.ls` also applied here, and this is the first program in
/// this module actually given a real argument, exercising `arg`'s bounds
/// check and `strlen` read together with `arg_count`'s own count rather
/// than only the no-argument path `assert_backends_agree` covers.
/// `binarytrees.ls` -- `arg_count`/`arg`'s other named target -- still
/// refuses past this: it also needs bare `Expr::Boxed` (§7.12's other
/// row), not scoped here.
#[test]
fn the_two_backends_agree_on_fannkuch() {
    for backend in ["cranelift", "llvm"] {
        let dir = scratch(&format!("backends-fannkuch-{backend}"));
        let exe = dir.join("out");
        let build = Command::new(BIN)
            .args([
                "build".as_ref(),
                repo_root().join("benches/game/fannkuch.ls").as_os_str(),
                "--std".as_ref(),
                "--backend".as_ref(),
                backend.as_ref(),
                "-o".as_ref(),
                exe.as_os_str(),
            ])
            .output()
            .expect("the compiler runs");
        assert!(
            build.status.success(),
            "`--backend {backend}` should build `fannkuch.ls`, but said:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );

        let output = Command::new(&exe).arg("8").output().expect("the program runs");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(output.status.code(), Some(0), "`--backend {backend}` should exit 0");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "1616\nPfannkuchen(8) = 22\n",
            "`--backend {backend}` printed the wrong thing"
        );
    }
}

/// The boundary this slice draws is a located refusal, not a crash or a
/// silent wrong answer: `region`/`alloc_slice` moved out of this list
/// once §7.5 landed; `arena_roundtrip.ls` now refuses on bare `alloc[a]`
/// instead (a single-value arena allocation handing back a unique
/// reference, still not part of this backend).
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
    assert!(message.contains("alloc"), "the refusal should name the boundary it hit: {message}");
}
