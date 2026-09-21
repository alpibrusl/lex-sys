//! The M0 conformance harness.
//!
//! Every fixture under `tests/accept/` must compile, run, and produce the
//! output its header declares. Every fixture under `tests/reject/` must be
//! refused, with the message its header declares.
//!
//! The header syntax is a comment the compiler ignores:
//!
//! ```text
//! //~ STDIN <a line fed to the program>          (default: nothing)
//! //~ STDOUT <a line the program must print>
//! //~ EXIT <the status it must exit with>      (default 0)
//! //~ ERROR <a substring the refusal must contain>
//! ```
//!
//! `STDIN` arrived with `docs/standard-input.md`: a fixture that reads
//! input needs input to be tested with, and every runner here fed a
//! program nothing. It is read from the same header as the rest, so the
//! accept walker and the example walker got it at the same moment.
//!
//! Adding a rule to the language means adding a fixture here. M2 says every
//! rule needs a must-reject fixture (#1, #2); the discipline starts at M0, when
//! it is cheap.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_lex-sys");

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("the workspace root is two levels above this crate")
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lex-sys-conformance-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a writable temporary directory");
    dir
}

fn fixtures(kind: &str) -> Vec<PathBuf> {
    let dir = repo_root().join("tests").join(kind);
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read `{}`: {e}", dir.display()))
        .map(|entry| entry.expect("a readable directory entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "ls"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no fixtures in `{}`", dir.display());
    paths
}

/// Collect the `//~ <key> <value>` directives from a fixture's header.
fn directives(source: &str, key: &str) -> Vec<String> {
    let prefix = format!("//~ {key} ");
    source
        .lines()
        .filter_map(|line| line.trim().strip_prefix(&prefix).map(|rest| rest.to_owned()))
        .collect()
}

fn directive(source: &str, key: &str) -> Option<String> {
    directives(source, key).into_iter().next()
}

/// Run a compiled fixture, feeding it whatever its header's `STDIN` lines
/// say (`docs/standard-input.md` §5).
///
/// Closing the pipe is the point rather than an implementation detail: a
/// program reading to end of input never ends until the writer hangs up,
/// so a fixture with no `STDIN` gets an immediately-closed stream rather
/// than an inherited terminal. That is what makes "reads until the input
/// ends" testable at all.
fn run_with_stdin(exe: &Path, source: &str) -> std::process::Output {
    let lines = directives(source, "STDIN");
    let mut child = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the compiled program runs");
    {
        let mut pipe = child.stdin.take().expect("a piped stdin");
        for line in &lines {
            writeln!(pipe, "{line}").expect("the program accepts its input");
        }
    }
    child.wait_with_output().expect("the program finishes")
}

#[test]
fn accepted_programs_build_and_run() {
    for path in fixtures("accept") {
        let source = std::fs::read_to_string(&path).expect("a readable fixture");
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let dir = scratch(&name);
        let exe = dir.join(&name);

        // `--std` for every accept fixture, for the same reason the
        // example walker passes it: a declaration nobody calls emits
        // nothing (`docs/standard-library.md` §5.2), so it costs the
        // fixtures that ignore it exactly nothing.
        let build = Command::new(BIN)
            .args([
                "build".as_ref(),
                path.as_os_str(),
                "--std".as_ref(),
                "-o".as_ref(),
                exe.as_os_str(),
            ])
            .output()
            .expect("the compiler runs");
        assert!(
            build.status.success(),
            "`{name}` should compile, but the compiler said:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );

        let run = run_with_stdin(&exe, &source);

        let mut expected = directives(&source, "STDOUT").join("\n");
        if !expected.is_empty() {
            expected.push('\n');
        }
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            expected,
            "`{name}` printed the wrong thing"
        );

        let expected_status: i32 =
            directive(&source, "EXIT").map_or(0, |s| s.trim().parse().expect("a numeric EXIT"));
        assert_eq!(run.status.code(), Some(expected_status), "`{name}` exited wrongly");

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn refused_programs_are_refused_with_the_stated_reason() {
    for path in fixtures("reject") {
        let source = std::fs::read_to_string(&path).expect("a readable fixture");
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let expected = directives(&source, "ERROR");
        assert!(!expected.is_empty(), "`{name}` declares no expected error");

        let output = Command::new(BIN).arg("check").arg(&path).output().expect("the compiler runs");

        assert_eq!(
            output.status.code(),
            Some(1),
            "`{name}` should be refused with exit code 1, got {:?}\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );

        let stderr = String::from_utf8_lossy(&output.stderr);
        for fragment in expected {
            assert!(
                stderr.contains(&fragment),
                "`{name}` should have been refused with `{fragment}`, but said:\n{stderr}"
            );
        }
        // A refusal is always located: path, line and column.
        assert!(
            stderr.contains(&format!("{}:", path.display())) || stderr.contains(".ls:"),
            "`{name}` was refused without a location:\n{stderr}"
        );
    }
}

/// Every example must build, run, and print what its header says.
///
/// A walker rather than one test per example, so an example added later is
/// covered without anyone remembering to cover it — and so an example that
/// stops matching the language fails CI instead of quietly rotting.
#[test]
fn every_example_runs_and_prints_what_it_says() {
    let dir = repo_root().join("examples");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read `{}`: {e}", dir.display()))
        .map(|entry| entry.expect("a readable directory entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "ls"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no examples in `{}`", dir.display());

    for path in paths {
        let source = std::fs::read_to_string(&path).expect("a readable example");
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        assert!(
            !directives(&source, "STDOUT").is_empty(),
            "`{name}` declares no expected output; every example states what it prints"
        );

        let scratch = scratch(&format!("example-{name}"));
        let exe = scratch.join(&name);
        // Every example is built with the standard library available
        // (`docs/standard-library.md` §2). Passing it unconditionally is
        // safe precisely because of §5.2 -- a declaration nobody calls
        // emits nothing, and `std_declarations_cost_nothing_unless_called`
        // is that as a test rather than as a hope.
        let build = Command::new(BIN)
            .args([
                "build".as_ref(),
                path.as_os_str(),
                "--std".as_ref(),
                "-o".as_ref(),
                exe.as_os_str(),
            ])
            .output()
            .expect("the compiler runs");
        assert!(
            build.status.success(),
            "`{name}` should compile, but the compiler said:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );

        let run = run_with_stdin(&exe, &source);
        let mut expected = directives(&source, "STDOUT").join("\n");
        expected.push('\n');
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            expected,
            "`{name}` printed the wrong thing"
        );

        let expected_status: i32 =
            directive(&source, "EXIT").map_or(0, |s| s.trim().parse().expect("a numeric EXIT"));
        assert_eq!(run.status.code(), Some(expected_status), "`{name}` exited wrongly");

        let _ = std::fs::remove_dir_all(&scratch);
    }
}

#[test]
fn run_builds_and_executes_in_one_step() {
    let source = repo_root().join("examples").join("hello.ls");
    let output = Command::new(BIN).arg("run").arg(&source).output().expect("the compiler runs");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Hello, world!\n");
    assert_eq!(output.status.code(), Some(0));
}

/// The canonical printer's two contracts, over every `.ls` file in the repo.
///
/// 1. **Identity-preserving.** Parsing the printed text gives back the same
///    hash for every declaration. That is what makes it the rendering step
///    of a store that addresses code by hash rather than merely a
///    pretty-printer.
/// 2. **Idempotent.** Printing the output again changes nothing, so the
///    canonical form is a fixed point.
///
/// Run over the accept fixtures, the examples *and* the reject fixtures --
/// the last of those parse even though they are refused later, and they are
/// where the odd syntax lives, so they are the most valuable input of the
/// three.
#[test]
fn printing_preserves_every_identity_and_is_idempotent() {
    let mut checked = 0;
    // `examples/wordfreq` is listed separately because the walkers here
    // filter on the `.ls` extension, which a directory does not have --
    // that is what keeps a multi-file example out of the single-file
    // harnesses, and it would keep it out of this one too.
    for dir in [
        "tests/accept",
        "tests/reject",
        "examples",
        "examples/wordfreq",
        "examples/buffer",
        "examples/slab",
        "examples/modular",
        "examples/serve",
        // The benchmarks are code too, and the pairs are the place a
        // careless edit would land without anyone reading it.
        "benches",
        "examples/base64",
        "examples/sort",
        // The standard library is code, and gets the same contract every
        // other file here gets: printed, reparsed, identical hashes, and
        // a fixed point.
        "std",
    ] {
        for entry in std::fs::read_dir(repo_root().join(dir)).expect("a readable directory") {
            let path = entry.expect("a readable entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("ls") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("a readable fixture");
            // A reject fixture may be refused by the *parser*, in which case
            // there is no tree to print and nothing to check here.
            let Ok(ast) = lex_sys_syntax::parse(&source) else { continue };

            let printed = lex_sys_syntax::print(&ast);
            let reparsed = lex_sys_syntax::parse(&printed).unwrap_or_else(|d| {
                panic!(
                    "{}: printed output does not parse: {}\n{printed}",
                    path.display(),
                    d.message
                )
            });

            let before = lex_sys_id::identify(&ast);
            let after = lex_sys_id::identify(&reparsed);
            assert_eq!(
                before.functions.len(),
                after.functions.len(),
                "{}: a declaration went missing",
                path.display()
            );
            for (a, b) in before.functions.iter().zip(after.functions.iter()) {
                assert_eq!(a.sig, b.sig, "{}: `{}`'s signature changed", path.display(), a.name);
                assert_eq!(a.body, b.body, "{}: `{}`'s body changed", path.display(), a.name);
            }
            for (a, b) in before.types.iter().zip(after.types.iter()) {
                assert_eq!(a.id, b.id, "{}: `{}` changed", path.display(), a.name);
            }

            let again = lex_sys_syntax::print(&reparsed);
            assert_eq!(printed, again, "{}: printing is not a fixed point", path.display());
            checked += 1;
        }
    }
    assert!(checked > 100, "the walk should have found the whole suite, found {checked}");
}

#[test]
fn byte_of_traps_outside_a_byte_rather_than_truncating() {
    // `docs/strings.md` §2: truncation is the silently wrong answer
    // `defined-behaviour.md` §2.1 already refused for `+`. One unsigned
    // comparison covers both ends, so `byte_of(-1)` dies with `byte_of(256)`.
    for value in ["256", "0 - 1"] {
        let dir = scratch(&format!("byte-range-{}", value.replace([' ', '-'], "")));
        let source = dir.join("byte.ls");
        std::fs::write(
            &source,
            format!(
                "fn main(world: World) -> [] int {{\n\
                     let Split {{ io, ffi, fs, heap, args }} = split(world); release(args); release(heap); release(fs); release(ffi); release(io);\n\
                     return int_of(byte_of({value}));\n\
                 }}\n"
            ),
        )
        .expect("a writable fixture");
        let exe = dir.join("byte");

        let build = Command::new(BIN)
            .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
            .output()
            .expect("the compiler runs");
        assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

        let run = Command::new(&exe).output().expect("the compiled program runs");
        assert!(!run.status.success(), "`byte_of({value})` should not succeed");
        assert_eq!(
            run.status.code(),
            None,
            "`byte_of({value})` should be killed by a signal, not exit"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn indexing_past_a_slice_traps_rather_than_reading_on() {
    // `docs/defined-behaviour.md` §1: the alternative to a bounds check is
    // reading past the end of an allocation, and this language has no
    // undefined behaviour to do that in. One unsigned comparison covers
    // both ends -- a negative index read as unsigned is enormous -- so the
    // check below catches `xs[5]` and `xs[-1]` with the same instruction.
    for index in ["5", "0 - 1"] {
        let dir = scratch(&format!("slice-bounds-{}", index.replace([' ', '-'], "")));
        let source = dir.join("bounds.ls");
        std::fs::write(
            &source,
            format!(
                "fn main(world: World) -> [] int {{\n\
                     let Split {{ io, ffi, fs, heap, args }} = split(world); release(args); release(heap); release(fs); release(ffi); release(io);\n\
                     var n = 0;\n\
                     region a {{ let xs = alloc_slice[a](3, 7); n = xs[{index}]; }}\n\
                     return n;\n\
                 }}\n"
            ),
        )
        .expect("a writable fixture");
        let exe = dir.join("bounds");

        let build = Command::new(BIN)
            .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
            .output()
            .expect("the compiler runs");
        assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

        let run = Command::new(&exe).output().expect("the compiled program runs");
        assert!(!run.status.success(), "`xs[{index}]` should not succeed");
        assert_eq!(run.status.code(), None, "`xs[{index}]` should be killed by a signal, not exit");

        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// `docs/authority.md` §2: the report names what a program performs.
///
/// Three examples whose surfaces differ, so this fails if the union is
/// taken over the wrong set rather than merely if it is empty.
#[test]
fn the_authority_report_names_what_a_program_performs() {
    let cases: &[(&str, &[&str], &[&str])] = &[
        // The console, both directions, and nothing else.
        (
            "tally.ls",
            &["io_read", "io_write"],
            &["the filesystem", "the heap", "the command line", "foreign code"],
        ),
        // Writes only.
        (
            "hello.ls",
            &["io_write"],
            &["the filesystem", "the heap", "the command line", "foreign code"],
        ),
        // Calls into C, and the symbol is named.
        ("pipeline.ls", &["ffi(\"libc\")", "io_write", "labs"], &["the filesystem"]),
    ];

    for (name, performs, never) in cases {
        let path = repo_root().join("examples").join(name);
        let out = Command::new(BIN)
            .args(["authority".as_ref(), path.as_os_str(), "--std".as_ref()])
            .output()
            .expect("the compiler runs");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let text = String::from_utf8_lossy(&out.stdout);
        for label in *performs {
            assert!(text.contains(label), "`{name}` should perform `{label}`:\n{text}");
        }
        for what in *never {
            assert!(text.contains(what), "`{name}` should never touch {what}:\n{text}");
        }
    }
}

/// §2.1, and the bug that section records.
///
/// `examples/lines.ls` reads `argv` in `main`'s **own body**, and `main`
/// declares `[]` because it owns its capabilities rather than borrowing
/// them. A report built from declared rows said *never touches the
/// command line* about the repository's command-line tool.
#[test]
fn the_authority_report_sees_what_main_does_itself() {
    let path = repo_root().join("examples").join("lines.ls");
    let out = Command::new(BIN)
        .args(["authority".as_ref(), path.as_os_str(), "--std".as_ref()])
        .output()
        .expect("the compiler runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("args"), "`lines.ls` reads argv in `main`:\n{text}");
    assert!(!text.contains("the command line"), "`lines.ls` does touch the command line:\n{text}");
}

/// §2.2: an absent label is a proof rather than an absence of evidence —
/// the capability was released, and nothing creates another.
#[test]
fn an_unused_capability_never_appears() {
    let dir = scratch("authority-negative");
    let source = dir.join("quiet.ls");
    // Releases everything and performs nothing at all.
    std::fs::write(
        &source,
        "fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(args); release(ffi); release(fs); release(heap); release(io);\n\
             return 0;\n\
         }\n",
    )
    .expect("a writable fixture");

    let out = Command::new(BIN)
        .args(["authority".as_ref(), source.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("performs nothing"), "{text}");
    for what in ["the console", "the filesystem", "the heap", "the command line", "foreign code"] {
        assert!(text.contains(what), "`{what}` should be listed as untouched:\n{text}");
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/budget.md` §5: the report as data, in the shape a supervisor
/// checks against a grant.
///
/// `effects` is the distinct kinds — the coarse question — and `labels`
/// keeps the narrowing for the precise one, which is the line
/// `lex-os-check`'s `CheckReport` already draws for Lex programs.
#[test]
fn the_authority_report_has_a_machine_readable_form() {
    let path = repo_root().join("examples").join("tour.ls");
    let out = Command::new(BIN)
        .args([
            "authority".as_ref(),
            path.as_os_str(),
            "--std".as_ref(),
            "--output".as_ref(),
            "json".as_ref(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);

    // Parsed rather than pattern-matched, so a malformed document fails
    // here rather than in whatever reads it next. No JSON dependency in
    // this crate, so the check is structural: balanced braces and
    // brackets, the three keys, and the narrowing carried through.
    assert_eq!(text.matches('{').count(), text.matches('}').count(), "unbalanced braces:\n{text}");
    assert_eq!(
        text.matches('[').count(),
        text.matches(']').count(),
        "unbalanced brackets:\n{text}"
    );
    for key in ["\"effects\"", "\"labels\"", "\"foreign_symbols\""] {
        assert!(text.contains(key), "missing {key}:\n{text}");
    }
    // The coarse kind and the precise argument, both present and distinct.
    assert!(text.contains("\"fs_read\""), "{text}");
    assert!(
        text.contains("{ \"name\": \"fs_read\", \"argument\": \"/tmp\" }"),
        "the narrowing should survive:\n{text}"
    );
    // A label that was never narrowed carries an explicit null rather
    // than being absent, so a consumer never has to tell the two apart.
    assert!(
        text.contains("{ \"name\": \"heap\", \"argument\": null }"),
        "an unnarrowed label needs an explicit null:\n{text}"
    );
    assert!(text.contains("\"labs\""), "the foreign symbol should be named:\n{text}");

    // And the human form is unchanged by the flag's existence.
    let plain = Command::new(BIN)
        .args(["authority".as_ref(), path.as_os_str(), "--std".as_ref()])
        .output()
        .expect("the compiler runs");
    assert!(plain.status.success());
    let plain = String::from_utf8_lossy(&plain.stdout);
    assert!(plain.contains("fs_read(\"/tmp\")"), "{plain}");
}

/// `docs/slicing.md` §2: a range past the end traps.
///
/// The same rule indexing has, applied to the operation that produces a
/// range rather than an element -- and it has to be, because the
/// alternative is a slice claiming a length its allocation does not
/// have, which is a buffer overrun with a type on it.
#[test]
fn slicing_past_the_end_traps() {
    // One unsigned comparison covers both ends, as it does for an index:
    // a negative bound read as unsigned is enormous.
    for range in ["0..13", "0 - 1..3"] {
        let tag = format!("slice-bounds-{}", range.replace([' ', '-', '.'], ""));
        let dir = scratch(&tag);
        let source = dir.join("bounds.ls");
        std::fs::write(
            &source,
            format!(
                "fn main(world: World) -> [] int {{\n\
                     let Split {{ io, ffi, fs, heap, args }} = split(world); release(args); release(heap); release(fs); release(ffi); release(io);\n\
                     let text = \"hello, world\";\n\
                     return len(text[{range}]);\n\
                 }}\n"
            ),
        )
        .expect("a writable fixture");
        let exe = dir.join("bounds");
        let build = Command::new(BIN)
            .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
            .output()
            .expect("the compiler runs");
        assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

        let run = Command::new(&exe).output().expect("the compiled program runs");
        assert_eq!(
            run.status.code(),
            None,
            "`text[{range}]` should be killed by a signal, not exit"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// §2 again: `a > b` stops rather than yielding empty.
///
/// An inverted range is a bug in the program that wrote it, and quietly
/// returning nothing is the defined-but-wrong answer
/// `defined-behaviour.md` §2.1 refuses.
#[test]
fn an_inverted_range_traps() {
    let dir = scratch("slice-inverted");
    let source = dir.join("inverted.ls");
    std::fs::write(
        &source,
        "fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(fs); release(ffi); release(io);\n\
             let text = \"hello, world\";\n\
             return len(text[5..2]);\n\
         }\n",
    )
    .expect("a writable fixture");
    let exe = dir.join("inverted");
    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert_eq!(run.status.code(), None, "`text[5..2]` should be killed by a signal");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn integer_overflow_traps_rather_than_wrapping() {
    // `docs/defined-behaviour.md` §2.1. Wrapping would be *defined* -- C has
    // it for unsigned, Rust has it in release -- so it is not undefined
    // behaviour that is being refused here, it is a silently wrong answer.
    // The wrong answer propagates; the stopped process does not.
    let dir = scratch("integer-overflow");
    let source = dir.join("overflow.ls");
    std::fs::write(
        &source,
        "fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(fs); release(ffi); release(io);\n\
             var n = 9223372036854775807;\n\
             return n + 1;\n\
         }\n",
    )
    .expect("a writable fixture");
    let exe = dir.join("overflow");

    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert!(!run.status.success(), "overflow should not succeed");
    assert_eq!(run.status.code(), None, "the process should be killed by a signal, not exit");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn exhausting_an_arena_traps_rather_than_running_past_the_chunk() {
    // §6: an arena is one chunk, obtained once and released once, which is
    // what makes release O(1). Asking it for more than it has is therefore
    // possible -- and it *traps*, because the alternative to a trap is
    // writing past the end of an allocation, and this language does not have
    // undefined behaviour to do that in (#1).
    let dir = scratch("arena-exhaustion");
    let source = dir.join("exhaust.ls");
    std::fs::write(
        &source,
        "struct Node { value: int }\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(fs); release(ffi); release(io);\n\
             region a {\n\
                 var i = 0;\n\
                 while i < 20000 {\n\
                     let node = alloc[a](Node { value: i });\n\
                     i = i + 1;\n\
                 }\n\
             }\n\
             return 0;\n\
         }\n",
    )
    .expect("a writable fixture");
    let exe = dir.join("exhaust");

    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert!(!run.status.success(), "an exhausted arena should not succeed");
    assert_eq!(run.status.code(), None, "the process should be killed by a signal, not exit");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn division_by_zero_traps_rather_than_being_undefined() {
    let dir = scratch("divide-by-zero");
    let source = dir.join("divzero.ls");
    std::fs::write(
        &source,
        "fn divide(a: int, b: int) -> [] int { return a / b; }\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(fs); release(ffi);\n\
             release(io);\n\
             return divide(1, 0);\n\
         }\n",
    )
    .expect("a writable fixture");
    let exe = dir.join("divzero");

    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    // A trap, not a silently wrong answer and not undefined behaviour: the
    // process dies rather than continuing with nonsense (#1).
    assert!(!run.status.success(), "division by zero should not succeed");
    assert_eq!(run.status.code(), None, "the process should be killed by a signal, not exit");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_path_outside_the_granted_prefix_traps() {
    // `docs/filesystem.md` §4. The prefix lives in the type and is known at
    // compile time; the path is a runtime slice, because a program that
    // could not name a file at run time could not be a tool. So the check
    // happens where the path is, and a path outside what the capability
    // granted *traps* -- it is not a missing file, it is a program doing
    // something its own type said it would not.
    let dir = scratch("fs-outside-prefix");
    let source = dir.join("outside.ls");
    std::fs::write(
        &source,
        "fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(ffi); release(io);\n\
             let tmp = narrow(fs, \"/tmp/lex-sys-granted\");\n\
             var read = 0;\n\
             region a {\n\
                 let buffer = alloc_slice[a](16, byte_of(0));\n\
                 borrow tmp as &f in {\n\
                     read = fs_read(f, \"/etc/hostname\", buffer);\n\
                 }\n\
             }\n\
             release(tmp);\n\
             return read;\n\
         }\n",
    )
    .expect("a writable fixture");

    let exe = dir.join("outside");
    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert!(!run.status.success(), "a path outside the prefix should not succeed");
    assert_eq!(run.status.code(), None, "the process should be killed by a signal, not exit");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_path_containing_dot_dot_traps() {
    // §4.1. A prefix check on bytes is defeated by `/tmp/../etc/passwd`,
    // and there are two honest answers: normalise the path, or refuse it.
    // Normalisation is a security function with a long history of being got
    // wrong and needs its own design, symlinks included -- so M3 refuses,
    // visibly, rather than shipping a check that quietly does not hold.
    let dir = scratch("fs-dot-dot");
    let source = dir.join("traversal.ls");
    std::fs::write(
        &source,
        "fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(ffi); release(io);\n\
             let tmp = narrow(fs, \"/tmp\");\n\
             var read = 0;\n\
             region a {\n\
                 let buffer = alloc_slice[a](16, byte_of(0));\n\
                 borrow tmp as &f in {\n\
                     read = fs_read(f, \"/tmp/../etc/hostname\", buffer);\n\
                 }\n\
             }\n\
             release(tmp);\n\
             return read;\n\
         }\n",
    )
    .expect("a writable fixture");

    let exe = dir.join("traversal");
    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert!(!run.status.success(), "a path containing `..` should not succeed");
    assert_eq!(run.status.code(), None, "the process should be killed by a signal, not exit");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sibling_of_the_granted_directory_traps() {
    // §1, at run time this time. `/tmp/lex-sys-granted` does not contain
    // `/tmp/lex-sys-granted-elsewhere`, however many bytes the two names
    // share. The compile-time refusal (`tests/reject/fs_sibling_prefix.ls`)
    // covers the same rule for the *prefix*; this covers it for the path,
    // which is the half nobody can see before the program runs.
    let dir = scratch("fs-sibling");
    let source = dir.join("sibling.ls");
    std::fs::write(
        &source,
        "fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(ffi); release(io);\n\
             let tmp = narrow(fs, \"/tmp/lex-sys-granted\");\n\
             var read = 0;\n\
             region a {\n\
                 let buffer = alloc_slice[a](16, byte_of(0));\n\
                 borrow tmp as &f in {\n\
                     read = fs_read(f, \"/tmp/lex-sys-granted-elsewhere\", buffer);\n\
                 }\n\
             }\n\
             release(tmp);\n\
             return read;\n\
         }\n",
    )
    .expect("a writable fixture");

    let exe = dir.join("sibling");
    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert!(!run.status.success(), "a sibling of the granted directory should not succeed");
    assert_eq!(run.status.code(), None, "the process should be killed by a signal, not exit");

    let _ = std::fs::remove_dir_all(&dir);
}

/// A program that echoes `argc` and every argument, one per line.
fn echo_source() -> String {
    "fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io_write] int {\n\
         var n = 0;\n\
         while n < len(s) { putchar(io, int_of(s[n])); n = n + 1; }\n\
         return len(s);\n\
     }\n\
     fn print_nat[&i](io: &!i Io, n: int) -> [io_write] int {\n\
         if n >= 10 { print_nat(io, n / 10); }\n\
         return putchar(io, 48 + n % 10);\n\
     }\n\
     fn run[&a, &i](args: &a Args, io: &!i Io) -> [args, io_write] int {\n\
         let count = arg_count(args);\n\
         print_nat(io, count); putchar(io, 10);\n\
         var n = 1;\n\
         while n < count {\n\
             write_all(io, arg(args, n)); putchar(io, 10); n = n + 1;\n\
         }\n\
         return count;\n\
     }\n\
     fn main(world: World) -> [] int {\n\
         let Split { io, ffi, fs, heap, args } = split(world);\n\
         release(ffi); release(fs); release(heap);\n\
         var status = 0;\n\
         borrow args as &a in { borrow mut io as &!i in { status = run(a, i); } }\n\
         release(args); release(io);\n\
         return status - 1;\n\
     }\n"
    .to_owned()
}

#[test]
fn a_program_reads_the_arguments_it_was_started_with() {
    // `docs/arguments.md` §3. Only a *running* program with real arguments
    // can show this, so it cannot be a fixture: the accept harness passes
    // none.
    //
    // The interesting cases are the ones a naive implementation gets wrong.
    // An argument containing a space is one argument, not two, because the
    // shell already split them and the program is handed a vector. An empty
    // argument is still an argument and still counted. And the bytes come
    // back without C's NUL, because the terminator is an artifact of the
    // interface rather than part of the value (§3.2) -- which is why the
    // lines below are exactly as long as what was passed in.
    let dir = scratch("args-read");
    let source = dir.join("echo.ls");
    std::fs::write(&source, echo_source()).expect("a writable fixture");

    let exe = dir.join("echo");
    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe)
        .args(["alpha", "two words", "", "--flag=x"])
        .output()
        .expect("the compiled program runs");
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "5\nalpha\ntwo words\n\n--flag=x\n",
        "the bytes a program is started with are the bytes it reads"
    );
    // `argc` counts the program name, so five here: it is not hidden.
    assert_eq!(run.status.code(), Some(4), "argc should be 5, and 5 - 1 is the exit status");

    // With no arguments at all there is still one: the program's own name.
    let bare = Command::new(&exe).output().expect("the compiled program runs");
    assert_eq!(String::from_utf8_lossy(&bare.stdout), "1\n");
    assert_eq!(bare.status.code(), Some(0));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_argument_past_the_end_traps() {
    // §3: the same mistake as indexing past a slice, and the same answer.
    let dir = scratch("args-past-end");
    let source = dir.join("past.ls");
    std::fs::write(
        &source,
        "fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(ffi); release(fs); release(heap); release(io);\n\
             var n = 0;\n\
             borrow args as &a in { n = len(arg(a, arg_count(a))); }\n\
             release(args);\n\
             return n;\n\
         }\n",
    )
    .expect("a writable fixture");

    let exe = dir.join("past");
    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert!(!run.status.success(), "an argument past the end should not succeed");
    assert_eq!(run.status.code(), None, "the process should be killed by a signal, not exit");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Build a program from several named files, in the order given.
fn build_many(tag: &str, files: &[(&str, &str)]) -> (PathBuf, std::process::Output) {
    let dir = scratch(tag);
    let mut paths: Vec<PathBuf> = Vec::new();
    for (name, source) in files {
        let path = dir.join(name);
        std::fs::write(&path, source).expect("a writable fixture");
        paths.push(path);
    }
    let exe = dir.join("program");
    let mut command = Command::new(BIN);
    command.arg("build");
    for path in &paths {
        command.arg(path);
    }
    command.arg("-o").arg(&exe);
    let build = command.output().expect("the compiler runs");
    (exe, build)
}

const UTIL_LS: &str = "fn print_nat[&i](io: &!i Io, n: int) -> [io_write] int {\n\
                           if n >= 10 { print_nat(io, n / 10); }\n\
                           return putchar(io, 48 + n % 10);\n\
                       }\n";

#[test]
fn the_multi_file_example_builds_and_runs() {
    // `examples/wordfreq` is the capstone: more than one file, arguments,
    // file IO, the heap, matching through references, and slices, each
    // doing real work rather than being demonstrated.
    //
    // The single-file example harness cannot reach it -- it filters on the
    // `.ls` extension and a directory has none -- so it is run here.
    let root = repo_root().join("examples").join("wordfreq");
    let dir = scratch("wordfreq");
    let exe = dir.join("wordfreq");
    let build = Command::new(BIN)
        .arg("build")
        .arg(root.join("main.ls"))
        .arg(root.join("text.ls"))
        .arg(root.join("counts.ls"))
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    // With no arguments it counts its own sample. The list is built by
    // prepending, so the order is reverse first-seen.
    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "dog 1\nlazy 1\nover 1\njumps 1\nfox 2\nbrown 1\nquick 1\nthe 3\n"
    );
    assert_eq!(run.status.code(), Some(0), "eight distinct words");

    // Given a path it counts that file instead.
    let doc = dir.join("doc.txt");
    std::fs::write(&doc, "alpha beta alpha\ngamma beta alpha\n").expect("a writable fixture");
    let counted = Command::new(&exe).arg(&doc).output().expect("the compiled program runs");
    assert_eq!(String::from_utf8_lossy(&counted.stdout), "gamma 1\nbeta 2\nalpha 3\n");

    let _ = std::fs::remove_dir_all(&dir);
}

/// A module's whole cost to the identity system, which is nothing
/// (`docs/modules.md` §2).
///
/// `canonical-ast.md` §1 has said since M0 that "moving a function
/// between files changes nothing about it". A module could have broken
/// that, and did not -- because a call already encodes the callee's
/// **hash** rather than its spelling, for an unrelated reason.
///
/// So this compiles the same two functions twice: once flat, once with
/// the callee in a module and the caller reaching it through an import.
/// All four hashes must be identical. Not similar -- the same.
#[test]
fn moving_a_function_into_a_module_changes_no_hash() {
    let ids = |tag: &str, files: &[(&str, &str)]| -> String {
        let dir = scratch(tag);
        let mut command = Command::new(BIN);
        command.arg("ids");
        for (name, source) in files {
            let path = dir.join(name);
            std::fs::write(&path, source).expect("a writable fixture");
            command.arg(path);
        }
        let out = command.output().expect("the compiler runs");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        // Sorted, because the two programs list their declarations in
        // different orders and this is a claim about hashes, not order.
        let mut lines: Vec<String> =
            String::from_utf8_lossy(&out.stdout).lines().map(str::to_owned).collect();
        lines.sort();
        let _ = std::fs::remove_dir_all(&dir);
        lines.join("\n")
    };

    let flat = ids(
        "modules-identity-flat",
        &[(
            "flat.ls",
            "fn twice(n: int) -> [] int { return n + n; }\n\
             fn caller() -> [] int { return twice(21); }\n",
        )],
    );
    let modular = ids(
        "modules-identity-modular",
        &[
            ("user.ls", "import m;\nfn caller() -> [] int { return m.twice(21); }\n"),
            ("lib.ls", "module m;\npub fn twice(n: int) -> [] int { return n + n; }\n"),
        ],
    );

    assert_eq!(flat, modular, "a module reached the hash, and it must not");
}

/// The multi-file half of `docs/modules.md` §8's suite.
///
/// These need two files each, so they cannot be `tests/reject/` fixtures
/// -- that walker compiles one file at a time. Same discipline all the
/// same: every rule in the document has a program that breaks it, and the
/// message it is refused with is written down.
#[test]
fn the_module_rules_are_enforced_across_files() {
    const LIB: &str = "module lib;\n\
                       pub fn shown() -> [] int { return 1; }\n\
                       fn hidden() -> [] int { return 2; }\n\
                       struct Secret { n: int }\n\
                       pub struct Open { n: int }\n";

    let refused = |tag: &str, main: &str| -> String {
        let (_, build) = build_many(tag, &[("main.ls", main), ("lib.ls", LIB)]);
        assert!(!build.status.success(), "`{tag}` should have been refused");
        String::from_utf8_lossy(&build.stderr).into_owned()
    };

    // §5: private is private.
    let private = refused(
        "modules-private",
        "import lib;\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(args); release(ffi); release(fs); release(heap); release(io);\n\
             return lib.hidden();\n\
         }\n",
    );
    assert!(private.contains("`hidden` is not `pub`"), "{private}");

    // §5, for a type rather than a function.
    let private_type = refused(
        "modules-private-type",
        "import lib;\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(args); release(ffi); release(fs); release(heap); release(io);\n\
             let s: lib.Secret = lib.Secret { n: 1 };\n\
             return s.n;\n\
         }\n",
    );
    assert!(private_type.contains("`Secret` is not `pub`"), "{private_type}");

    // §4.1: an import binds a qualifier, not a set of names. `shown` is
    // `pub` and imported, and still not in scope unqualified.
    let unqualified = refused(
        "modules-unqualified",
        "import lib;\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(args); release(ffi); release(fs); release(heap); release(io);\n\
             return shown();\n\
         }\n",
    );
    assert!(unqualified.contains("`shown` is not a function"), "{unqualified}");

    // §4: a qualified name that is not there is an error, never a
    // fall back to the local module. `elsewhere` is defined right here.
    let missing = refused(
        "modules-missing",
        "import lib;\n\
         fn elsewhere() -> [] int { return 3; }\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(args); release(ffi); release(fs); release(heap); release(io);\n\
             return lib.elsewhere();\n\
         }\n",
    );
    assert!(missing.contains("`elsewhere` is not a function"), "{missing}");

    // §4: two imports may not bind one qualifier.
    let (_, collision) = build_many(
        "modules-collision",
        &[
            (
                "main.ls",
                "import lib;\n\
                 import other.lib;\n\
                 fn main(world: World) -> [] int {\n\
                     let Split { io, ffi, fs, heap, args } = split(world);\n\
                     release(args); release(ffi); release(fs); release(heap); release(io);\n\
                     return 0;\n\
                 }\n",
            ),
            ("lib.ls", LIB),
            ("other.ls", "module other.lib;\npub fn nothing() -> [] int { return 0; }\n"),
        ],
    );
    assert!(!collision.status.success(), "two imports bound `lib`");
    let text = String::from_utf8_lossy(&collision.stderr);
    assert!(text.contains("is already bound to another import"), "{text}");

    // And the same program with an `as` is accepted, which is what makes
    // the refusal above a rule rather than a limit.
    let (_, renamed) = build_many(
        "modules-renamed",
        &[
            (
                "main.ls",
                "import lib;\n\
                 import other.lib as other;\n\
                 fn main(world: World) -> [] int {\n\
                     let Split { io, ffi, fs, heap, args } = split(world);\n\
                     release(args); release(ffi); release(fs); release(heap); release(io);\n\
                     return lib.shown() + other.nothing() - 1;\n\
                 }\n",
            ),
            ("lib.ls", LIB),
            ("other.ls", "module other.lib;\npub fn nothing() -> [] int { return 0; }\n"),
        ],
    );
    assert!(renamed.status.success(), "{}", String::from_utf8_lossy(&renamed.stderr));
}

/// `docs/collections.md` §5: a `match` names an enum through the same
/// qualifier every other reference uses.
///
/// This is not decoration. `Pattern::Variant` carried no qualifier, so a
/// `match` could only name an enum its own module declared — which makes
/// an imported enum a type a program can hold, pass around and **never
/// take apart**. `std.option` is unusable without this, and so is every
/// enum any library will ever export.
#[test]
fn a_match_names_an_enum_through_its_qualifier() {
    const SHAPES: &str = "module shapes;\n\
                          pub enum Shape { Flat, Tall(int) }\n\
                          pub fn tall(n: int) -> [] Shape { return Shape::Tall(n); }\n";

    let (exe, build) = build_many(
        "modules-qualified-pattern",
        &[
            (
                "main.ls",
                "import shapes;\n\
                 fn height(s: shapes.Shape) -> [] int {\n\
                     match s {\n\
                         shapes.Shape::Flat => { return 0; }\n\
                         shapes.Shape::Tall(n) => { return n; }\n\
                     }\n\
                 }\n\
                 fn main(world: World) -> [] int {\n\
                     let Split { io, ffi, fs, heap, args } = split(world);\n\
                     release(args); release(ffi); release(fs); release(heap); release(io);\n\
                     return height(shapes.tall(7)) + height(shapes.Shape::Flat) - 7;\n\
                 }\n",
            ),
            ("shapes.ls", SHAPES),
        ],
    );
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert_eq!(run.status.code(), Some(0));

    // And the qualifier is checked rather than decorative: a name that is
    // not an import here is an error, not something skipped over because
    // the scrutinee already said which enum this is.
    let (_, wrong) = build_many(
        "modules-qualified-pattern-unbound",
        &[
            (
                "main.ls",
                "import shapes;\n\
                 fn height(s: shapes.Shape) -> [] int {\n\
                     match s {\n\
                         forms.Shape::Flat => { return 0; }\n\
                         forms.Shape::Tall(n) => { return n; }\n\
                     }\n\
                 }\n\
                 fn main(world: World) -> [] int {\n\
                     let Split { io, ffi, fs, heap, args } = split(world);\n\
                     release(args); release(ffi); release(fs); release(heap); release(io);\n\
                     return height(shapes.Shape::Flat);\n\
                 }\n",
            ),
            ("shapes.ls", SHAPES),
        ],
    );
    assert!(!wrong.status.success(), "`forms` is not imported");
    let text = String::from_utf8_lossy(&wrong.stderr);
    assert!(text.contains("`forms` is not an imported module here"), "{text}");
}

/// `examples/modular/` — two modules and a root, with a qualifier, an
/// `as`, a private helper and a module importing another.
#[test]
fn the_modular_example_builds_and_runs() {
    let root = repo_root().join("examples").join("modular");
    let dir = scratch("modular-example");
    let exe = dir.join("modular");
    let build = Command::new(BIN)
        .arg("build")
        .arg(root.join("main.ls"))
        .arg(root.join("counts.ls"))
        .arg(root.join("text.ls"))
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "seen 3, total 60\n60\n");
    assert_eq!(run.status.code(), Some(0));

    let _ = std::fs::remove_dir_all(&dir);
}

/// The standard library type-checks on its own, with no program
/// (`docs/standard-library.md` §7).
///
/// Named on the command line like any other module -- which is the point
/// of `modules.md`: the library is not special, it is just files whose
/// source happens to ship in the compiler.
#[test]
fn the_standard_library_compiles_on_its_own() {
    let root = repo_root().join("std");
    let mut command = Command::new(BIN);
    command.arg("check");
    for name in
        ["bytes.ls", "math.ls", "io.ls", "buffer.ls", "option.ls", "result.ls", "list.ls", "vec.ls"]
    {
        command.arg(root.join(name));
    }
    let out = command.output().expect("the compiler runs");
    // No `main`, so the CLI refuses at the end -- but only after every
    // declaration has been checked, which is what this is asserting.
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(
        text.contains("no `main` function"),
        "the library itself should check clean; got:\n{text}"
    );
}

/// `--std` makes the library's source present without naming a file
/// (§2), and it is still opt-in: the program writes its own `import`.
#[test]
fn std_is_available_behind_a_flag() {
    let dir = scratch("std-flag");
    let source = dir.join("tool.ls");
    std::fs::write(
        &source,
        "import std.io;\n\
         import std.bytes;\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(args); release(ffi); release(fs); release(heap);\n\
             borrow mut io as &!i in {\n\
                 io.print_pad(i, 0 - 42, 6);\n\
                 io.newline(i);\n\
             }\n\
             release(io);\n\
             return bytes.digit_of(55) - 7;\n\
         }\n",
    )
    .expect("a writable fixture");
    let exe = dir.join("tool");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            source.as_os_str(),
            "--std".as_ref(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "   -42\n");
    assert_eq!(run.status.code(), Some(0), "`digit_of('7')` is 7");

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/standard-library.md` §5.2: a declaration nobody calls costs
/// nothing.
///
/// The same program built with `--std` and without it emits
/// **byte-identical** object files. Not smaller-by-a-bit -- the same
/// bytes, because emission is driven by what `main` reaches and an
/// unreached declaration is still checked and never lowered.
///
/// This claim was **false** when it was first written down, which is why
/// it is a test: the library added 5.6 KB to a program that called none
/// of it, because pass 2 seeded from every non-generic function rather
/// than from the entry point.
#[test]
fn std_declarations_cost_nothing_unless_called() {
    const BARE: &str = "fn main(world: World) -> [] int {\n\
                            let Split { io, ffi, fs, heap, args } = split(world);\n\
                            release(args); release(ffi); release(fs); release(heap);\n\
                            borrow mut io as &!i in { putchar(i, 65); }\n\
                            release(io);\n\
                            return 0;\n\
                        }\n";
    let dir = scratch("std-costs-nothing");
    let source = dir.join("bare.ls");
    std::fs::write(&source, BARE).expect("a writable fixture");

    let object = |name: &str, extra: &[&str]| -> Vec<u8> {
        let out = dir.join(name);
        let mut command = Command::new(BIN);
        command.arg("build").arg(&source);
        for flag in extra {
            command.arg(flag);
        }
        command.arg("--emit").arg("obj").arg("-o").arg(&out);
        let build = command.output().expect("the compiler runs");
        assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
        std::fs::read(&out).expect("a readable object file")
    };

    assert_eq!(
        object("without.o", &[]),
        object("with.o", &["--std"]),
        "the standard library reached the output of a program that never calls it"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `std.math` §3.3: `abs` traps on the most negative integer.
///
/// Every other language's `abs` returns the negative number here, which
/// is the silently-wrong answer this one exists to refuse. It does not
/// return at all -- and the trap is `0 - n` doing what `-` already does
/// rather than a check bolted on, so it costs nothing on every other
/// input.
#[test]
fn abs_of_the_most_negative_integer_traps() {
    let dir = scratch("std-abs-traps");
    let source = dir.join("abs.ls");
    std::fs::write(
        &source,
        "import std.math;\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(args); release(ffi); release(fs); release(heap); release(io);\n\
             return math.abs(-9223372036854775808);\n\
         }\n",
    )
    .expect("a writable fixture");
    let exe = dir.join("abs");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            source.as_os_str(),
            "--std".as_ref(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert!(!run.status.success(), "`abs(int::MIN)` must not succeed");
    assert_eq!(run.status.code(), None, "it is killed by a signal, not an exit");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_program_can_be_spread_over_several_files() {
    // `docs/many-files.md` §2: a program is a set of files, named on the
    // command line in any order, sharing one flat namespace.
    //
    // `main.ls` calls `print_nat`, which is declared in a file listed
    // *after* it, and `twice`, declared in a third. Order does not matter
    // because there is no order to matter: the files are one program.
    let (exe, build) = build_many(
        "many-files",
        &[
            (
                "main.ls",
                "fn main(world: World) -> [] int {\n\
                     let Split { io, ffi, fs, heap, args } = split(world);\n\
                     release(args); release(heap); release(fs); release(ffi);\n\
                     borrow mut io as &!i in { print_nat(i, twice(21)); putchar(i, 10); }\n\
                     release(io);\n\
                     return twice(21) - 42;\n\
                 }\n",
            ),
            ("util.ls", UTIL_LS),
            ("math.ls", "fn twice(n: int) -> [] int { return n + n; }\n"),
        ],
    );
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
    assert_eq!(run.status.code(), Some(0));

    let _ = std::fs::remove_dir_all(exe.parent().expect("a directory"));
}

#[test]
fn a_diagnostic_names_the_file_it_came_from() {
    // §4: spans are offsets into the whole program's source, and a
    // `SourceMap` resolves one back to a file, a line and a column. The
    // error here is in the *third* file, several thousand bytes into the
    // program, and has to be reported at that file's own line 1.
    let dir = scratch("many-files-diagnostic");
    let main = dir.join("main.ls");
    let util = dir.join("util.ls");
    let broken = dir.join("broken.ls");
    std::fs::write(
        &main,
        "fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(args); release(heap); release(fs); release(ffi); release(io);\n\
             return 0;\n\
         }\n",
    )
    .expect("a writable fixture");
    std::fs::write(&util, UTIL_LS).expect("a writable fixture");
    std::fs::write(&broken, "fn oops() -> [] int { return missing(); }\n")
        .expect("a writable fixture");

    let output = Command::new(BIN)
        .args(["check".as_ref(), main.as_os_str(), util.as_os_str(), broken.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(&format!("{}:1:", broken.display())), "{stderr}");
    assert!(stderr.contains("`missing` is not a function"), "{stderr}");
    // The offending source line, from the right file.
    assert!(stderr.contains("fn oops() -> [] int"), "{stderr}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_name_is_declared_once_per_program_not_per_file() {
    // §2.2: the namespace is flat and shared, so a duplicate across two
    // files is the same error as a duplicate within one. Nothing new had
    // to be invented -- this is `duplicate_function.ls` noticing a second
    // file.
    let (_, build) = build_many(
        "many-files-duplicate",
        &[
            (
                "main.ls",
                "fn twice(n: int) -> [] int { return n + n; }\n\
                 fn main(world: World) -> [] int {\n\
                     let Split { io, ffi, fs, heap, args } = split(world);\n\
                     release(args); release(heap); release(fs); release(ffi); release(io);\n\
                     return twice(0);\n\
                 }\n",
            ),
            ("other.ls", "fn twice(n: int) -> [] int { return n * 2; }\n"),
        ],
    );
    assert_eq!(build.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&build.stderr);
    assert!(stderr.contains("twice"), "{stderr}");

    let _ = std::fs::remove_dir_all(scratch("many-files-duplicate"));
}

#[test]
fn identity_is_content_not_location() {
    // §3, and the reason that section exists. `canonical-ast.md` §1 has
    // claimed since M0 that "moving a function between files changes
    // nothing about it". With one file there were no files to move
    // between; with several there are, so it is checked.
    //
    // The same function, in two programs, at different positions, with
    // different neighbours, in differently named files: same `SigId`,
    // same `BodyId`.
    let dir = scratch("many-files-identity");
    let alone = dir.join("alone.ls");
    let crowded = dir.join("crowded.ls");
    let body = "fn double(n: int) -> [] int { return n + n; }\n";
    let main = "fn main(world: World) -> [] int {\n\
                    let Split { io, ffi, fs, heap, args } = split(world);\n\
                    release(args); release(heap); release(fs); release(ffi); release(io);\n\
                    return double(0);\n\
                }\n";
    std::fs::write(&alone, format!("{body}{main}")).expect("a writable fixture");
    std::fs::write(
        &crowded,
        format!("fn unrelated(n: int) -> [] int {{ return n * 3; }}\n{body}{main}"),
    )
    .expect("a writable fixture");

    let ids_of = |path: &Path| -> String {
        let out = Command::new(BIN).arg("ids").arg(path).output().expect("the compiler runs");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| l.contains("double"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let first = ids_of(&alone);
    assert!(!first.is_empty(), "`double` should have hashes");
    assert_eq!(first, ids_of(&crowded), "a unit hashes its content, not where it sits");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_boxed_slice_checks_its_own_size() {
    // `docs/boxed-slices.md` §3. Two runtime rules, both of which would
    // otherwise reserve less memory than is about to be written.
    //
    // A negative count is not a small allocation, it is a mistake -- and
    // `s[0]` of one would read memory nobody reserved. A `count * stride`
    // that overflows is the same mistake arrived at by arithmetic, which
    // is why it is checked for the reason every other multiplication is.
    for count in ["0 - 3", "4611686018427387904"] {
        let dir = scratch("boxed-slice-size");
        let source = dir.join("size.ls");
        std::fs::write(
            &source,
            format!(
                "fn main(world: World) -> [] int {{\n\
                     let Split {{ io, ffi, fs, heap, args }} = split(world);\n\
                     release(args); release(fs); release(ffi); release(io);\n\
                     var n = 0;\n\
                     borrow mut heap as &!h in {{\n\
                         let b = box_slice(h, {count}, 0);\n\
                         n = unbox_slice(h, b);\n\
                     }}\n\
                     release(heap);\n\
                     return n;\n\
                 }}\n"
            ),
        )
        .expect("a writable fixture");

        let exe = dir.join("size");
        let build = Command::new(BIN)
            .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
            .output()
            .expect("the compiler runs");
        assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

        let run = Command::new(&exe).output().expect("the compiled program runs");
        assert_eq!(
            run.status.code(),
            None,
            "`box_slice(h, {count}, 0)` should be killed by a signal, not exit"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn the_growable_buffer_example_builds_and_runs() {
    // `examples/buffer/` is the library `docs/boxed-slices.md` §4
    // describes: growing is allocate-copy-end, written down rather than
    // built in, so the doubling policy belongs to the program.
    let root = repo_root().join("examples").join("buffer");
    let dir = scratch("buffer-example");
    let exe = dir.join("buffer");
    let build = Command::new(BIN)
        .arg("build")
        .arg(root.join("main.ls"))
        .arg(root.join("buffer.ls"))
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "counting: 1 4 9 16 25 36 49 64\n");
    assert_eq!(run.status.code(), Some(0), "thirty-one bytes built from a one-byte buffer");

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/tuples.md` §5: a tuple is a struct with the names removed, so
/// replacing one with the other changes no generated code.
///
/// Stated that way it is a claim about a compiler, and the strongest form
/// of it available is the one asserted here: the two programs below differ
/// only in whether the pair is a declared `res struct` or a tuple, and
/// their **object files are byte-identical**. Not similar, not the same
/// size -- the same bytes.
///
/// That is what makes tuples an ergonomic feature rather than a
/// representation choice, and it is why `examples/slab/` could drop two
/// declared types without anyone having to ask what it cost.
#[test]
fn a_tuple_emits_the_same_object_as_the_struct_it_replaces() {
    const STRUCT: &str = "\
res struct Pair { held: Box[int], tag: int }
fn make[&h](heap: &!h Heap, n: int) -> [heap] Pair {
    return Pair { held: box(heap, n), tag: n + 1 };
}
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(ffi); release(fs); release(io);
    var status = 0;
    borrow mut heap as &!h in {
        let p = make(h, 41);
        let Pair { held, tag } = p;
        status = unbox(h, held) + tag;
    }
    release(heap);
    return status - 83;
}
";
    const TUPLE: &str = "\
fn make[&h](heap: &!h Heap, n: int) -> [heap] (Box[int], int) {
    return (box(heap, n), n + 1);
}
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(ffi); release(fs); release(io);
    var status = 0;
    borrow mut heap as &!h in {
        let p = make(h, 41);
        let (held, tag) = p;
        status = unbox(h, held) + tag;
    }
    release(heap);
    return status - 83;
}
";

    let dir = scratch("tuple-layout");
    let mut objects = Vec::new();
    for (name, source) in [("declared", STRUCT), ("anonymous", TUPLE)] {
        let path = dir.join(format!("{name}.ls"));
        std::fs::write(&path, source).expect("a writable fixture");
        let object = dir.join(format!("{name}.o"));
        let build = Command::new(BIN)
            .args([
                "build".as_ref(),
                path.as_os_str(),
                "--emit".as_ref(),
                "obj".as_ref(),
                "-o".as_ref(),
                object.as_os_str(),
            ])
            .output()
            .expect("the compiler runs");
        assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
        objects.push(std::fs::read(&object).expect("a readable object file"));
    }

    assert_eq!(
        objects[0], objects[1],
        "a tuple and the struct it replaces must emit the same object file"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_slab_example_builds_and_runs() {
    // `examples/slab/` is §9's `Gen` hatch, built as `docs/sharing.md` §3
    // describes it. The line that matters is the last one: a handle whose
    // slot was removed comes back `Missing` -- a value, not a dangling
    // pointer -- and the program decides what to do about it.
    //
    // The three `rc_*.ls` reject fixtures are the other half of the same
    // claim: `Gen` is a library, and `Rc` is not one that can be written.
    let root = repo_root().join("examples").join("slab");
    let dir = scratch("slab-example");
    let exe = dir.join("slab");
    let build = Command::new(BIN)
        .arg("build")
        .arg(root.join("main.ls"))
        .arg(root.join("slab.ls"))
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "live handle:  7\nafter remove: missing\nnew handle:   9\nold handle:   missing\n"
    );
    assert_eq!(run.status.code(), Some(0), "one slot live at the end, so `drop_slab` said 1");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_heap_actually_frees() {
    // `docs/heap.md` §3.1 claims the general heap cannot leak. The checker
    // guarantees `unbox` runs on every path, but that is a claim about the
    // *program* -- this is the claim about the emitted code.
    //
    // Eight million boxes of 2 KiB each, one at a time. Freeing makes the
    // footprint one box; leaking makes it 16 GB, which no machine this runs
    // on has. So a regression that dropped the `free` does not produce a
    // subtly worse number here, it fails: either our own `trapz` fires when
    // `malloc` returns null, or the process is killed. Both are a non-zero
    // exit, and both are what this asserts against.
    //
    // (Run under valgrind on linux-x86_64 while this was written: one
    // million allocs, one million frees, "in use at exit: 0 bytes in 0
    // blocks". Valgrind is not on both CI targets, so the portable check is
    // the one above.)
    const FIELDS: usize = 256;
    const ROUNDS: usize = 8_000_000;

    let fields = (0..FIELDS).map(|i| format!("f{i}: int")).collect::<Vec<_>>().join(", ");
    let init = (0..FIELDS).map(|i| format!("f{i}: 1")).collect::<Vec<_>>().join(", ");

    let dir = scratch("heap-frees");
    let source = dir.join("churn.ls");
    std::fs::write(
        &source,
        format!(
            "struct Chunk {{ {fields} }}\n\
             fn churn[&h](heap: &!h Heap, rounds: int) -> [heap] int {{\n\
                 var total = 0;\n\
                 var i = 0;\n\
                 while i < rounds {{\n\
                     let b = box(heap, Chunk {{ {init} }});\n\
                     let c = unbox(heap, b);\n\
                     total = total + c.f0;\n\
                     i = i + 1;\n\
                 }}\n\
                 return total;\n\
             }}\n\
             fn main(world: World) -> [] int {{\n\
                 let Split {{ io, ffi, fs, heap, args }} = split(world); release(args);\n\
                 release(ffi); release(fs); release(io);\n\
                 var total = 0;\n\
                 borrow mut heap as &!h in {{ total = churn(h, {ROUNDS}); }}\n\
                 release(heap);\n\
                 return total - {ROUNDS};\n\
             }}\n"
        ),
    )
    .expect("a writable fixture");

    let exe = dir.join("churn");
    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert_eq!(
        run.status.code(),
        Some(0),
        "eight million boxes in a bounded footprint should succeed; a leak would need 16 GB"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_written_file_is_readable_by_its_owner() {
    // The regression guard for a real bug, and the reason it is worth a test
    // of its own rather than leaving it to `file_roundtrip.ls`.
    //
    // `open` is variadic -- `int open(const char *, int, ...)` -- and on
    // Apple ARM64 a variadic argument travels on the stack while a fixed one
    // travels in a register. Calling it with three *fixed* arguments
    // therefore created files with whatever mode happened to be on the
    // stack: the write succeeded and reported the right byte count, and the
    // file was unreadable afterwards. Linux x86-64 cannot see this, because
    // there varargs and fixed arguments share the same registers.
    //
    // So the mode is checked directly, from outside the program, rather than
    // inferred from a read that happens to succeed.
    let dir = scratch("fs-mode");
    let source = dir.join("mode.ls");
    let target = dir.join("written.txt");
    let path = target.to_string_lossy().into_owned();
    std::fs::write(
        &source,
        format!(
            "fn main(world: World) -> [] int {{\n\
                 let Split {{ io, ffi, fs, heap, args }} = split(world); release(args); release(heap); release(ffi); release(io);\n\
                 let one = narrow(fs, \"{path}\");\n\
                 var wrote = 0;\n\
                 borrow one as &f in {{ wrote = fs_write(f, \"{path}\", \"written\\n\"); }}\n\
                 release(one);\n\
                 return wrote - 8;\n\
             }}\n"
        ),
    )
    .expect("a writable fixture");

    let exe = dir.join("mode");
    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert_eq!(run.status.code(), Some(0), "the write should have reported 8 bytes");

    let written = std::fs::read(&target).expect("the file the program wrote is readable");
    assert_eq!(written, b"written\n");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&target).expect("the file exists").permissions().mode();
        // `creat` asks for 0644 and the umask may clear group and other
        // bits, but never the owner's. A mode that lost them is the bug.
        assert_eq!(mode & 0o600, 0o600, "created with mode {:o}", mode & 0o777);
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_file_is_minus_one_rather_than_a_trap() {
    // §3. The distinction `defined-behaviour.md` draws everywhere: `-1` for
    // an outcome a program should handle, a trap for a broken promise. A
    // file that is not there is the first kind.
    let dir = scratch("fs-missing");
    let source = dir.join("missing.ls");
    std::fs::write(
        &source,
        "fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(ffi); release(io);\n\
             let tmp = narrow(fs, \"/tmp/lex-sys-not-here\");\n\
             var read = 0;\n\
             region a {\n\
                 let buffer = alloc_slice[a](16, byte_of(0));\n\
                 borrow tmp as &f in {\n\
                     read = fs_read(f, \"/tmp/lex-sys-not-here/at-all\", buffer);\n\
                 }\n\
             }\n\
             release(tmp);\n\
             return 0 - read;\n\
         }\n",
    )
    .expect("a writable fixture");

    let exe = dir.join("missing");
    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert_eq!(run.status.code(), Some(1), "a missing file should return -1, not trap");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn emitting_a_bare_object_file_works() {
    let dir = scratch("emit-obj");
    let object = dir.join("hello.o");
    let source = repo_root().join("examples").join("hello.ls");

    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            source.as_os_str(),
            "--emit".as_ref(),
            "obj".as_ref(),
            "-o".as_ref(),
            object.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    assert!(std::fs::metadata(&object).expect("an object file").len() > 0);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ids_are_stable_across_runs_and_survive_a_body_rewrite() {
    let source = repo_root().join("examples").join("rational.ls");

    let once = Command::new(BIN).arg("ids").arg(&source).output().expect("the compiler runs");
    assert!(once.status.success(), "{}", String::from_utf8_lossy(&once.stderr));
    let again = Command::new(BIN).arg("ids").arg(&source).output().expect("the compiler runs");
    assert_eq!(once.stdout, again.stdout, "hashing is a function of the program alone");

    let text = String::from_utf8(once.stdout).expect("hashes are ascii");
    assert!(text.contains("sig  harmonic"), "{text}");
    assert!(text.contains("type Rational"), "{text}");

    // Rewrite a body without touching any signature: every `sig` line must be
    // unchanged and at least one `body` line must move.
    let dir = scratch("ids-rewrite");
    let rewritten = dir.join("rational.ls");
    let original = std::fs::read_to_string(&source).expect("a readable example");
    let patched = original.replace(
        "fn abs(x: int) -> [] int {\n    if x < 0 {\n        return 0 - x;\n    }\n    return x;\n}",
        "fn abs(x: int) -> [] int {\n    if x >= 0 {\n        return x;\n    }\n    return 0 - x;\n}",
    );
    assert_ne!(patched, original, "the body rewrite should have applied");
    std::fs::write(&rewritten, patched).expect("a writable copy");

    let after = Command::new(BIN).arg("ids").arg(&rewritten).output().expect("the compiler runs");
    assert!(after.status.success(), "{}", String::from_utf8_lossy(&after.stderr));
    let after = String::from_utf8(after.stdout).expect("hashes are ascii");

    let sigs = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|l| l.starts_with("sig ") || l.starts_with("type "))
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(sigs(&text), sigs(&after), "a body rewrite must not move any signature");
    assert_ne!(text, after, "it must move a body");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_wrong_command_line_is_a_usage_error_not_a_refusal() {
    let output = Command::new(BIN).arg("frobnicate").output().expect("the compiler runs");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
}

/// `examples/serve/` — a REST endpoint, answered over a real TCP socket.
///
/// This is the test `docs/reach.md` rests on. The document's claim is that
/// a lex-sys program can serve HTTP **today**, with no socket type, no
/// `Net` capability and no library, and a claim like that is worth exactly
/// what it is tested with. So the test is not "it compiles": it builds the
/// program, runs it, connects to it from this process over loopback, and
/// reads what comes back.
///
/// It runs twice because the server answers one request and exits, which
/// is what makes it testable without a shutdown protocol — the second run
/// is the 404, and the two together are the router.
///
/// The program takes its port from `argv`, so this picks one the operating
/// system says is free rather than hard-coding a number that CI might
/// already be using.
#[test]
fn an_http_server_written_in_lex_sys_answers_a_real_request() {
    use std::io::{Read, Write as _};
    use std::net::{TcpListener, TcpStream};
    use std::time::{Duration, Instant};

    let scratch = scratch("example-serve");
    let exe = scratch.join("serve");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            repo_root().join("examples/serve/serve.ls").as_os_str(),
            "--std".as_ref(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(
        build.status.success(),
        "`serve` should compile, but the compiler said:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    // `[(request target, status line, body)]` — one exchange per run.
    let exchanges = [
        ("/health", "HTTP/1.1 200 OK", "{\"ok\":true}"),
        ("/nothing-here", "HTTP/1.1 404 Not Found", "{\"error\":\"not found\"}"),
    ];

    for (target, status_line, body) in exchanges {
        // Bind and drop: the port is free at this instant, which is the
        // best any test can say about a port it did not get from the
        // program itself.
        let port = TcpListener::bind("127.0.0.1:0")
            .expect("a free loopback port")
            .local_addr()
            .expect("a bound address")
            .port();

        let mut child = Command::new(&exe)
            .arg(port.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the compiled server runs");

        // The server is a process, so "listening" is not an event this
        // test can observe — it retries the connection until the listen
        // backlog exists or the deadline passes.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(s) => break s,
                Err(e) if Instant::now() < deadline => {
                    let _ = e;
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(e) => {
                    let _ = child.kill();
                    panic!("the server never accepted a connection on {port}: {e}");
                }
            }
        };

        stream.set_read_timeout(Some(Duration::from_secs(10))).expect("a readable socket");
        write!(stream, "GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .expect("the server accepts a request");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("the server answers");

        assert!(
            response.starts_with(status_line),
            "`serve` answered `{target}` with the wrong status:\n{response}"
        );
        assert!(
            response.ends_with(&format!("\r\n\r\n{body}")),
            "`serve` answered `{target}` with the wrong body:\n{response}"
        );
        // The length it declares and the bytes it sends come from the same
        // slice, and this is that being true rather than assumed.
        assert!(
            response.contains(&format!("Content-Length: {}\r\n", body.len())),
            "`serve` declared a length that is not the body's:\n{response}"
        );

        let run = child.wait_with_output().expect("the server exits");
        assert_eq!(run.status.code(), Some(0), "`serve` exited wrongly after one request");
    }

    let _ = std::fs::remove_dir_all(&scratch);
}

/// And what the authority report says about it, which is §5's whole point.
///
/// The row is `ffi("libc")` and nothing else: exact, and silent about the
/// network, because a library is not an authority domain. What covers the
/// difference is the foreign symbol list — `socket`, `bind`, `listen`,
/// `accept` are in the binary because `main` reaches them, and a
/// supervisor reading that list knows what it is being asked to run.
#[test]
fn the_authority_report_names_the_syscalls_the_row_cannot() {
    let output = Command::new(BIN)
        .args(["authority".as_ref(), repo_root().join("examples/serve/serve.ls").as_os_str()])
        .args(["--std", "--output", "json"])
        .output()
        .expect("the compiler runs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let report = String::from_utf8(output.stdout).expect("the report is utf-8");

    // Two capabilities, and it proves the absence of the other three.
    assert!(report.contains("\"effects\": [\"args\", \"ffi\"]"), "{report}");
    assert!(report.contains("{ \"name\": \"ffi\", \"argument\": \"libc\" }"), "{report}");

    // The row says "calls a C library". The symbols say which library
    // calls, and those are the ones that name a socket.
    for symbol in ["socket", "bind", "listen", "accept"] {
        assert!(
            report.contains(&format!("\"{symbol}\"")),
            "the report should name `{symbol}`:\n{report}"
        );
    }
}

/// `benches/` — both halves of every pair compute the same answer.
///
/// The benchmarks are how `docs/overflow-cost.md` knows what the overflow
/// trap costs, and the measurement is only meaningful while the two halves
/// are the same program. Each one returns `result - expected`, so a
/// non-zero exit is a wrong answer — which is what this checks.
///
/// **Not a timing gate.** Wall-clock in CI is noise, and a benchmark that
/// fails the build when a runner is busy teaches people to ignore it.
/// `scripts/bench.py` is where the numbers come from; this is only here so
/// a refactor cannot quietly make the two halves disagree.
#[test]
fn every_benchmark_pair_agrees() {
    let dir = repo_root().join("benches");
    let scratch = scratch("benches");
    let mut pairs = 0;

    for name in ["sum", "sieve", "scan", "fib"] {
        for half in ["checked", "wrapping"] {
            let source = dir.join(format!("{name}_{half}.ls"));
            let exe = scratch.join(format!("{name}_{half}"));
            let build = Command::new(BIN)
                .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
                .output()
                .expect("the compiler runs");
            assert!(
                build.status.success(),
                "`{name}_{half}` should compile, but the compiler said:\n{}",
                String::from_utf8_lossy(&build.stderr)
            );

            let run = Command::new(&exe).output().expect("the benchmark runs");
            assert_eq!(
                run.status.code(),
                Some(0),
                "`{name}_{half}` computed the wrong answer (it exits with its error)"
            );
        }
        pairs += 1;
    }

    assert_eq!(pairs, 4, "every pair in `benches/` should be covered here");
    let _ = std::fs::remove_dir_all(&scratch);
}

/// `docs/bitwise.md` §3 and §4 — what a shift does at the edges.
///
/// Not reject fixtures: a trapping program is one that compiled, and the
/// reject harness runs `check` (`slicing.md` §8 made the same correction).
///
/// The fourth case is the one that is easy to get wrong in the other
/// direction. `1 << 63` sets the sign bit, which read as arithmetic is an
/// overflow — and §4 says a shift is bits, so it is the answer.
#[test]
fn a_shift_past_the_width_traps() {
    let scratch = scratch("bitwise-shift");
    // (expression, does it trap). A trap is `ud2`, so the process is killed
    // by a signal and has no exit code — which is how every other trapping
    // test here states it.
    let cases = [
        ("1 << 64", true),
        ("1 >> 64", true),
        ("1 << (0 - 1)", true),
        ("1 >> (0 - 1)", true),
        // Cranelift's `ishl` masks the amount, so an unguarded lowering
        // would make this `1 << 1` and hand back 2. It traps instead.
        ("1 << 65", true),
        ("1 << 63", false),
        ("1 << 0", false),
        ("1 << 63 >> 63", false),
    ];

    for (index, (expression, traps)) in cases.iter().enumerate() {
        let source = format!(
            "fn main(world: World) -> [] int {{\n\
             \x20   let Split {{ io, ffi, fs, heap, args }} = split(world);\n\
             \x20   release(args); release(heap); release(fs); release(ffi); release(io);\n\
             \x20   var seen = 0;\n\
             \x20   if ({expression}) != 12345 {{ seen = 0; }}\n\
             \x20   return seen;\n\
             }}\n"
        );
        let path = scratch.join(format!("shift{index}.ls"));
        std::fs::write(&path, &source).expect("a writable fixture");
        let exe = scratch.join(format!("shift{index}"));
        let build = Command::new(BIN)
            .args(["build".as_ref(), path.as_os_str(), "-o".as_ref(), exe.as_os_str()])
            .output()
            .expect("the compiler runs");
        assert!(
            build.status.success(),
            "`{expression}` should compile — the amount is a runtime value, not a type error:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let run = Command::new(&exe).output().expect("the program runs");
        if *traps {
            assert_eq!(
                run.status.code(),
                None,
                "`{expression}` should be killed by a signal, not exit"
            );
        } else {
            assert_eq!(run.status.code(), Some(0), "`{expression}` should not trap");
        }
    }

    let _ = std::fs::remove_dir_all(&scratch);
}

/// `docs/bitwise.md` §6 — the operator set grew and no hash moved.
///
/// An operator is hashed by a code inside `Binary`/`Unary` rather than by
/// a node tag of its own, and the new codes were *appended*. So a program
/// written before this slice hashes to what it hashed before.
///
/// The values below were not recomputed after the change. They were taken
/// from a build of `83cc7a7`, the commit immediately before this slice, and
/// the two compilers print the same bytes for the same source — which is
/// what makes this a check rather than a restatement.
#[test]
fn ids_are_stable_across_the_operator_set() {
    // Deliberately uses only the operators that existed beforehand.
    let source = "fn mix(a: int, b: int) -> [] bool {\n\
                  \x20   return a + b * 2 - 1 < 10 && !(a == b);\n\
                  }\n";
    let scratch = scratch("ids-operator-set");
    let path = scratch.join("mix.ls");
    std::fs::write(&path, source).expect("a writable fixture");

    let output = Command::new(BIN).arg("ids").arg(&path).output().expect("the compiler runs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text = String::from_utf8(output.stdout).expect("hashes are ascii");

    // The whole point is that these are *literal* and were not recomputed
    // after the operators landed. If a future slice inserts an operator
    // code rather than appending one, this is what says so.
    assert!(
        text.contains("3c635be9a28ee7ea3f1f096db9c25f312d6ffd5f43c89e58820361e2e01f25fc"),
        "a signature moved; §6's append-only rule was broken\n{text}"
    );
    assert!(
        text.contains("d8f04b00ae8eef0ca032ba48f781d453a9f283116b410bd20c03a835f73fd1ad"),
        "a body moved; §6's append-only rule was broken\n{text}"
    );

    let _ = std::fs::remove_dir_all(&scratch);
}

/// `examples/base64/` — the port, checked against the program it ports.
///
/// `docs/porting.md`'s claim is that this is GNU coreutils' `base64`, and
/// a claim of that shape is worth what it is tested with. So the test
/// pipes the same bytes through both and compares: twelve input sizes
/// including the ones around the 76-column wrap, both directions, plus the
/// three malformed inputs where the exit status is the whole behaviour.
///
/// The comparison runs against **GNU** coreutils specifically, because
/// that is what `docs/porting.md` claims this is a port of, and the other
/// implementations disagree: macOS ships BSD `base64`, which prints a
/// newline for empty input where GNU prints nothing. Neither is wrong —
/// the port targets one of them, so the test compares against that one.
///
/// Where GNU `base64` is not on the path the comparison is skipped rather
/// than failed. Everything that needs no reference — the round trip, the
/// exit statuses, the megabyte — still runs on every platform, which is
/// why the darwin job is not simply doing less work.
#[test]
fn base64_agrees_with_coreutils() {
    let scratch = scratch("example-base64");
    let exe = scratch.join("base64");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            repo_root().join("examples/base64/base64.ls").as_os_str(),
            "--std".as_ref(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(
        build.status.success(),
        "`base64` should compile, but the compiler said:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    /// Feed `input` to `command` on stdin and hand back (stdout, exit code).
    ///
    /// The write goes on its own thread. Writing the whole input first and
    /// only then reading deadlocks as soon as the output exceeds a pipe
    /// buffer: the child blocks writing, so it stops reading, so the
    /// parent blocks writing. Base64 output is larger than its input, so
    /// this is reached by any case past about 48 KiB — which is exactly
    /// the megabyte case at the end, the one that is here to prove the
    /// program streams.
    fn pipe(command: &Path, args: &[&str], input: &[u8]) -> (Vec<u8>, Option<i32>) {
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the program runs");
        let mut stdin = child.stdin.take().expect("a piped stdin");
        let owned = input.to_vec();
        let writer = std::thread::spawn(move || {
            // A broken pipe is not a failure here: a program that refuses
            // its input exits before reading all of it, which is what the
            // malformed cases below are about.
            let _ = stdin.write_all(&owned);
            drop(stdin);
        });
        let out = child.wait_with_output().expect("it exits");
        writer.join().expect("the writer thread finishes");
        (out.stdout, out.status.code())
    }

    // Deterministic rather than random: a failing test should fail the same
    // way twice. The sizes are the boundaries — empty, each remainder mod
    // 3, and either side of the 57 input bytes that fill a 76-column line.
    let sizes = [0usize, 1, 2, 3, 4, 5, 17, 56, 57, 58, 100, 1000];
    let corpus: Vec<Vec<u8>> =
        sizes.iter().map(|n| (0..*n).map(|i| (i * 37 + i / 5) as u8).collect()).collect();

    // `base64 --version` prints "base64 (GNU coreutils) 9.4" on GNU. BSD's
    // has no `--version` at all and exits non-zero, which is the same
    // answer for this purpose: not the program that was ported.
    let reference = Path::new("/usr/bin/base64");
    let have_reference = reference.exists()
        && Command::new(reference)
            .arg("--version")
            // Null stdin, so a version probe can never end up waiting on
            // input it would otherwise inherit from the test runner.
            .stdin(Stdio::null())
            .output()
            .is_ok_and(|v| {
                v.status.success() && String::from_utf8_lossy(&v.stdout).contains("GNU coreutils")
            });

    for (size, input) in sizes.iter().zip(&corpus) {
        let (ours, status) = pipe(&exe, &[], input);
        assert_eq!(status, Some(0), "encoding {size} bytes should succeed");

        if have_reference {
            let (theirs, _) = pipe(reference, &[], input);
            assert_eq!(
                String::from_utf8_lossy(&ours),
                String::from_utf8_lossy(&theirs),
                "encoding {size} bytes differs from GNU coreutils"
            );
        }

        // The round trip holds with or without a reference to compare to.
        let (back, status) = pipe(&exe, &["-d"], &ours);
        assert_eq!(status, Some(0), "decoding {size} bytes should succeed");
        assert_eq!(&back, input, "the round trip lost {size} bytes");
    }

    // Malformed input, where the exit status *is* the behaviour.
    for bad in [&b"abc$def"[..], &b"QQ=A"[..], &b"Q"[..]] {
        let (_, status) = pipe(&exe, &["-d"], bad);
        assert_eq!(status, Some(1), "`{}` should be refused", String::from_utf8_lossy(bad));
        if have_reference {
            let (_, theirs) = pipe(reference, &["-d"], bad);
            assert_eq!(
                status,
                theirs,
                "GNU coreutils disagrees about `{}`",
                String::from_utf8_lossy(bad)
            );
        }
    }

    // An arena is one 64 KiB chunk, so a megabyte through it is the proof
    // that nothing buffers the input (`docs/porting.md` §4).
    let large: Vec<u8> = (0..1_048_576).map(|i| (i * 31 + i / 7) as u8).collect();
    let (encoded, status) = pipe(&exe, &[], &large);
    assert_eq!(status, Some(0), "a megabyte should encode");
    let (decoded, status) = pipe(&exe, &["-d"], &encoded);
    assert_eq!(status, Some(0), "a megabyte should decode");
    assert_eq!(decoded.len(), large.len(), "the round trip changed a megabyte's length");
    assert!(decoded == large, "the round trip changed a megabyte's contents");

    let _ = std::fs::remove_dir_all(&scratch);
}

/// `examples/sort/` — the second port, checked against GNU `sort`.
///
/// The comparison is `LC_ALL=C`, because that is the ordering the program
/// implements: byte order, with no locale anywhere in this language to
/// implement anything else. Same GNU-detection rule as the `base64` test,
/// for the same reason — BSD's `sort` is a different program.
///
/// Where a reference is absent the shape checks still run: the output is
/// still required to be a sorted permutation of the input's lines, which
/// is most of what "sorted" means and needs nobody else's binary.
#[test]
fn sort_agrees_with_gnu_sort() {
    use std::collections::BTreeMap;

    let scratch = scratch("example-sort");
    let exe = scratch.join("sort");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            repo_root().join("examples/sort/sort.ls").as_os_str(),
            "--std".as_ref(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(
        build.status.success(),
        "`sort` should compile, but the compiler said:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    /// Run with `input` on stdin and `args` on the command line.
    fn run(command: &Path, args: &[&Path], input: &str) -> (String, Option<i32>) {
        let mut child = Command::new(command)
            .args(args)
            .env("LC_ALL", "C")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the program runs");
        let mut stdin = child.stdin.take().expect("a piped stdin");
        let owned = input.to_owned();
        let writer = std::thread::spawn(move || {
            let _ = stdin.write_all(owned.as_bytes());
        });
        let out = child.wait_with_output().expect("it exits");
        writer.join().expect("the writer thread finishes");
        (String::from_utf8_lossy(&out.stdout).into_owned(), out.status.code())
    }

    /// How many times each line appears, so the output can be checked to
    /// be a rearrangement of the input rather than merely sorted.
    fn multiset(lines: &str) -> BTreeMap<&str, usize> {
        let mut counts = BTreeMap::new();
        // A trailing newline *ends* the last line rather than starting an
        // empty one, which is the one thing a plain `split` gets wrong.
        let body = lines.strip_suffix('\n').unwrap_or(lines);
        if !body.is_empty() {
            for line in body.split('\n') {
                *counts.entry(line).or_default() += 1;
            }
        }
        counts
    }

    let reference = Path::new("/usr/bin/sort");
    let have_reference = reference.exists()
        && Command::new(reference).arg("--version").stdin(Stdio::null()).output().is_ok_and(|v| {
            v.status.success() && String::from_utf8_lossy(&v.stdout).contains("GNU coreutils")
        });

    // The shapes that decide a line sort: nothing, no trailing newline,
    // blank lines, one line a prefix of another, bytes outside the
    // letters, and enough lines to be a real merge.
    let bulk: String = (0..5000)
        .map(|i| format!("{}{}\n", "xyzab".as_bytes()[i % 5] as char, (i * 7919) % 10007))
        .collect();
    let corpus = [
        "",
        "zebra\nant",
        "\n\nb\n\na\n",
        "ab\na\nabc\nb\n",
        "~\n!\nA\na\n0\n",
        "same\nsame\nsame\n",
        &bulk,
    ];

    for input in corpus {
        let (ours, status) = run(&exe, &[], input);
        assert_eq!(status, Some(0), "sorting should succeed");

        // A permutation of the input's lines, in non-descending order.
        // Both halves hold with or without a reference to compare
        // against, which is what the darwin job checks when the `sort`
        // on that machine is not GNU's.
        assert_eq!(
            multiset(&ours),
            multiset(input),
            "the output is not a permutation of the input:\n{ours}"
        );
        let got: Vec<&str> = ours.lines().collect();
        assert!(got.windows(2).all(|w| w[0] <= w[1]), "the output is not in byte order:\n{ours}");

        if have_reference {
            let (theirs, _) = run(reference, &[], input);
            assert_eq!(ours, theirs, "differs from GNU sort on input {input:?}");
        }
    }

    // Named files, several at once, and one that is not there.
    let one = scratch.join("one.txt");
    let two = scratch.join("two.txt");
    std::fs::write(&one, "pear\napple\n").expect("a writable fixture");
    std::fs::write(&two, "fig\nbanana\n").expect("a writable fixture");

    let (ours, status) = run(&exe, &[&one, &two], "");
    assert_eq!(status, Some(0), "sorting two files should succeed");
    assert_eq!(ours, "apple\nbanana\nfig\npear\n", "two files should sort together");
    if have_reference {
        let (theirs, _) = run(reference, &[&one, &two], "");
        assert_eq!(ours, theirs, "differs from GNU sort across two files");
    }

    let missing = scratch.join("not-here.txt");
    let (_, status) = run(&exe, &[&missing], "");
    assert_eq!(status, Some(2), "a missing file should exit 2, as GNU does");

    // Past the 64 KiB first read, so the doubling in `read_file` runs.
    let large: String = (0..40_000).map(|i| format!("line {:06}\n", (i * 31) % 40_000)).collect();
    let big = scratch.join("big.txt");
    std::fs::write(&big, &large).expect("a writable fixture");
    let (ours, status) = run(&exe, &[&big], "");
    assert_eq!(status, Some(0), "a file past the first read should succeed");
    assert_eq!(ours.lines().count(), 40_000, "it lost lines while growing");
    assert!(
        ours.lines().collect::<Vec<_>>().windows(2).all(|w| w[0] <= w[1]),
        "a file past the first read came out unsorted"
    );
    if have_reference {
        let (theirs, _) = run(reference, &[&big], "");
        assert_eq!(ours, theirs, "differs from GNU sort on a file past the first read");
    }

    let _ = std::fs::remove_dir_all(&scratch);
}
