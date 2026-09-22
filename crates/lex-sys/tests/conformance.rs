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
//! //~ STDERR <a line it must print as a diagnostic>  (default: none)
//! //~ EXIT <the status it must exit with>      (default 0)
//! //~ ERROR <a substring the refusal must contain>
//! //~ RULE <the rule tag the refusal must carry>
//! ```
//!
//! `STDIN` arrived with `docs/standard-input.md`: a fixture that reads
//! input needs input to be tested with, and every runner here fed a
//! program nothing. It is read from the same header as the rest, so the
//! accept walker and the example walker got it at the same moment.
//!
//! `STDERR` arrived with `docs/standard-error.md` and is checked even
//! when a fixture declares none, which is the half that matters: a
//! program writing to the wrong stream now fails a test rather than
//! passing one quietly, and that is what the two directives being
//! separate is *for* (§1.1).
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

        // Checked whether or not the fixture declares any: a diagnostic
        // on a program that should have been silent is the failure this
        // catches, and it is the one the old harness could not see.
        let mut expected_err = directives(&source, "STDERR").join("\n");
        if !expected_err.is_empty() {
            expected_err.push('\n');
        }
        assert_eq!(
            String::from_utf8_lossy(&run.stderr),
            expected_err,
            "`{name}` said the wrong thing on standard error"
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

        // …and carries the rule it enforces (`docs/agent-errors.md` §3).
        // Declared in the fixture rather than derived from the message,
        // which is the whole point: a fixture says which rule it is a
        // fixture *for*, and a checker that starts answering a different
        // one fails here instead of quietly retagging.
        let rule =
            directive(&source, "RULE").unwrap_or_else(|| panic!("`{name}` declares no `//~ RULE`"));
        let rule = rule.trim();
        let json = Command::new(BIN)
            .arg("check")
            .arg(&path)
            .args(["--output", "json"])
            .output()
            .expect("the compiler runs");
        let body = String::from_utf8_lossy(&json.stdout);
        let reported = body
            .lines()
            .find_map(|line| line.trim().strip_prefix("\"rule\": \""))
            .map(|rest| rest.trim_end_matches("\","))
            .unwrap_or_else(|| panic!("`{name}` reported no rule:\n{body}"));
        assert_eq!(reported, rule, "`{name}` is a fixture for `{rule}` and reported `{reported}`");
    }
}

/// `docs/agent-errors.md` §3.2: every rule in the catalogue is a rule
/// some fixture reaches.
///
/// The harness's own header has said *"adding a rule to the language
/// means adding a fixture here"* since M0. It was kept for 38 rules out
/// of 47 when the catalogue was first written down — and the gap was
/// invisible until then, because "every rule" was a claim about a set
/// nobody had enumerated. **A catalogue is what makes a coverage claim
/// falsifiable**, which is an argument for tagging that has nothing to
/// do with agents.
#[test]
fn every_rule_has_a_fixture() {
    use std::collections::BTreeSet;

    // One rule the single-file harness cannot reach by construction:
    // `pub` is about reaching *another* module, a file declares at most
    // one module, and a fixture here is one file. It is covered by
    // `the_module_rules_are_enforced_across_files`, which is where a
    // second file exists.
    const COVERED_ELSEWHERE: [&str; 1] = ["not-public"];

    let declared: BTreeSet<String> = fixtures("reject")
        .iter()
        .filter_map(|path| {
            let source = std::fs::read_to_string(path).ok()?;
            Some(directive(&source, "RULE")?.trim().to_owned())
        })
        .collect();

    let missing: Vec<&str> = lex_sys_syntax::Rule::ALL
        .iter()
        .map(|r| r.tag())
        .filter(|tag| !declared.contains(*tag) && !COVERED_ELSEWHERE.contains(tag))
        .collect();
    assert!(
        missing.is_empty(),
        "these rules exist and no fixture reaches them: {missing:?}\n\
         Add one under `tests/reject/`, or say here where it is covered instead."
    );

    // The other direction: a fixture declaring a tag the catalogue does
    // not have is a typo that would otherwise pass forever.
    let catalogue: BTreeSet<&str> = lex_sys_syntax::Rule::ALL.iter().map(|r| r.tag()).collect();
    for tag in &declared {
        assert!(catalogue.contains(tag.as_str()), "no rule is called `{tag}`");
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

        // Checked whether or not the fixture declares any: a diagnostic
        // on a program that should have been silent is the failure this
        // catches, and it is the one the old harness could not see.
        let mut expected_err = directives(&source, "STDERR").join("\n");
        if !expected_err.is_empty() {
            expected_err.push('\n');
        }
        assert_eq!(
            String::from_utf8_lossy(&run.stderr),
            expected_err,
            "`{name}` said the wrong thing on standard error"
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
        "benches/three",
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
        text.contains("{ \"name\": \"fs_read\", \"argument\": \"/tmp\", \"bounded\": true }"),
        "the narrowing should survive:\n{text}"
    );
    // A label that was never narrowed carries an explicit null rather
    // than being absent, so a consumer never has to tell the two apart.
    assert!(
        text.contains("{ \"name\": \"heap\", \"argument\": null, \"bounded\": true }"),
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

/// `docs/emitted-checks.md` §4 — the three division traps nothing tested.
///
/// `division_by_zero_traps_rather_than_being_undefined` above covers
/// `1 / 0` and predates this. The other three were rules in
/// `defined-behaviour.md` §2.3 that no test had ever run, and reading the
/// emitted code found one of them false — so these exist to make the
/// section falsifiable on **both** targets rather than on the one a
/// disassembly happened to be taken on.
///
/// The operands come out of variables, because literals are folded and a
/// certain trap written down is a compile error rather than a run
/// (`compile-time.md` §4). §4.2 is what happens when those two paths
/// disagree.
#[test]
fn the_other_division_traps() {
    let dir = scratch("division-traps");
    // `(source, should it die)`.
    let cases = [("1", "0", "%", true), ("-9223372036854775808", "-1", "/", true)];

    for (a, b, op, dies) in cases {
        let name = if op == "%" { "rem" } else { "div" };
        let source = dir.join(format!("{name}.ls"));
        std::fs::write(
            &source,
            format!(
                "fn op(a: int, b: int) -> [] int {{ return a {op} b; }}\n\
                 fn main(world: World) -> [] int {{\n\
                     let Split {{ io, ffi, fs, heap, args }} = split(world);\n\
                     release(args); release(heap); release(fs); release(ffi); release(io);\n\
                     var x = {a}; var y = {b};\n\
                     return op(x, y);\n\
                 }}\n"
            ),
        )
        .expect("a writable fixture");
        let exe = dir.join(name);
        let build = Command::new(BIN)
            .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
            .output()
            .expect("the compiler runs");
        assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

        let run = Command::new(&exe).output().expect("the compiled program runs");
        assert_eq!(
            run.status.code(),
            if dies { None } else { Some(0) },
            "`{a} {op} {b}` should {}",
            if dies { "be killed by a signal" } else { "exit 0" }
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/emitted-checks.md` §4.1 and §4.2 — `a % -1` is 0, both ways.
///
/// `defined-behaviour.md` §2.3 said `int::MIN % -1` traps, borrowing the
/// quotient's reason for an operator that produces no quotient. It does
/// not: the emitted code is `cmp $-1` and a `mov $0`, and 0 is the right
/// answer.
///
/// Both spellings in one program on purpose. The constant folder had
/// Rust's `checked_rem` rule and the backend had the hardware's, so the
/// same expression was a compile error written down and a 0 computed.
/// This fails if either half moves.
#[test]
fn a_remainder_by_minus_one_is_zero() {
    let dir = scratch("rem-minus-one");
    let source = dir.join("rem.ls");
    std::fs::write(
        &source,
        "fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(args); release(heap); release(fs); release(ffi); release(io);\n\
             let folded = -9223372036854775808 % -1;\n\
             var a = -9223372036854775808; var b = -1;\n\
             let computed = a % b;\n\
             return folded + computed;\n\
         }\n",
    )
    .expect("a writable fixture");
    let exe = dir.join("rem");

    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(
        build.status.success(),
        "folding `int::MIN % -1` should agree with running it:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert_eq!(run.status.code(), Some(0), "both spellings should answer 0");

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/emitted-checks.md` §1 — folding a `byte`-returning call.
///
/// A pure call on constant arguments is folded to a literal, and `Expr`
/// has no `byte` literal, so the fold used to put an `int`-shaped node
/// where the backend expects one machine byte. Each of the three
/// expressions below then failed the Cranelift verifier with no span and
/// no rule tag.
///
/// Kept as three because they fail in three different places — a widen, a
/// comparison and a return — and a repair that fixed one without the
/// others would look right.
#[test]
fn folding_a_byte_returning_call() {
    let dir = scratch("fold-byte");
    let source = dir.join("fold.ls");
    std::fs::write(
        &source,
        "fn g(n: int) -> [] byte { return byte_of(n); }\n\
         fn h() -> [] byte { return g(65); }\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(args); release(heap); release(fs); release(ffi); release(io);\n\
             var same = 0;\n\
             if g(65) == byte_of(66) { same = 1; }\n\
             return int_of(g(65)) + int_of(h()) - 130 + same;\n\
         }\n",
    )
    .expect("a writable fixture");
    let exe = dir.join("fold");

    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(
        build.status.success(),
        "a folded `byte` should keep its width:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert_eq!(run.status.code(), Some(0), "`'A'` twice is 130, and 65 is not 66");

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
    assert!(
        report.contains("{ \"name\": \"ffi\", \"argument\": \"libc\", \"bounded\": false }"),
        "{report}"
    );

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

    for name in ["sum", "sieve", "scan", "fib", "reduce"] {
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

    // Counted from the directory rather than written down twice: a pair
    // added to `benches/` and forgotten here would otherwise never run.
    let on_disk = std::fs::read_dir(&dir)
        .expect("benches/ is readable")
        .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
        .filter(|name| name.ends_with("_checked.ls"))
        .count();
    assert_eq!(pairs, on_disk, "every pair in `benches/` should be covered here");
    let _ = std::fs::remove_dir_all(&scratch);
}

/// `benches/guards.c` — all three modes of every kernel compute the same
/// answer.
///
/// `docs/check-cost.md` and `docs/poison.md` read a cost out of the gaps
/// between unchecked, trapping and poison, and a gap means "what the
/// check costs" only while the three are the same program. If a check
/// ever *fires*, the number measured is the trap rather than the check,
/// and the kernel's data needs fixing rather than its timing believing —
/// which already happened once, to `byte_of`, whose first fill held a
/// `-1`.
///
/// Poison has a second way to be wrong that trapping does not: its
/// operation has to stay **defined** where the check would have fired —
/// a masked shift, a truncated byte, a clamped conversion — so a
/// disagreement here is as likely to be a bad fallback as bad data.
///
/// Built with a small `ROUNDS`, because the shape of the loop is what the
/// measurement is about and that does not depend on how many times the
/// outer one goes round. Not a timing gate, for the reason
/// `every_benchmark_pair_agrees` gives.
#[test]
fn every_guard_kernel_agrees_across_its_three_modes() {
    let source = repo_root().join("benches").join("guards.c");
    let scratch = scratch("guards");
    let mut kernels = 0;

    for kernel in 0..=11 {
        let mut answers = Vec::new();
        for mode in 0..=2 {
            let exe = scratch.join(format!("k{kernel}m{mode}"));
            let cc = Command::new("cc")
                .args([
                    "-O2".as_ref(),
                    "-DROUNDS=2".as_ref(),
                    format!("-DKERNEL={kernel}").as_ref(),
                    format!("-DMODE={mode}").as_ref(),
                    source.as_os_str(),
                    "-o".as_ref(),
                    exe.as_os_str(),
                ])
                .output()
                .expect("a C compiler");
            assert!(
                cc.status.success(),
                "kernel {kernel} mode={mode} should compile:\n{}",
                String::from_utf8_lossy(&cc.stderr)
            );
            let run = Command::new(&exe).output().expect("the kernel runs");
            assert_eq!(
                run.status.code(),
                Some(0),
                "kernel {kernel} mode={mode} should exit 0; a check that fires means \
                 the data trips it, so the gap would measure the trap"
            );
            answers.push(String::from_utf8_lossy(&run.stdout).into_owned());
        }
        assert!(
            answers.windows(2).all(|pair| pair[0] == pair[1]),
            "kernel {kernel}: unchecked, trapping and poison must compute the same thing, \
             and they printed {answers:?}"
        );
        kernels += 1;
    }

    // One `noinline run` per kernel. Counting the `#elif KERNEL ==` lines
    // instead would over-count: `main` switches on the same macro to fill
    // each kernel's data.
    let text = std::fs::read_to_string(&source).expect("guards.c is readable");
    let on_disk = text.matches("__attribute__((noinline))").count();
    assert_eq!(kernels, on_disk, "every kernel in `guards.c` should be covered here");
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
    //
    // The amount comes through a `var` rather than as a literal, and that
    // is load-bearing now: `docs/compile-time.md` §4 refuses a shift
    // whose operands are both literals *at compile time*, so writing
    // `1 << 64` here would test the diagnostic instead of the trap.
    // `a_certain_trap_is_refused_at_compile_time` is the other half.
    let cases = [
        ("64", true),
        ("0 - 1", true),
        // Cranelift's `ishl` masks the amount, so an unguarded lowering
        // would make `1 << 65` into `1 << 1` and hand back 2. It traps
        // instead.
        ("65", true),
        ("63", false),
        ("0", false),
    ];

    for (index, (expression, traps)) in cases.iter().enumerate() {
        let source = format!(
            "fn main(world: World) -> [] int {{\n\
             \x20   let Split {{ io, ffi, fs, heap, args }} = split(world);\n\
             \x20   release(args); release(heap); release(fs); release(ffi); release(io);\n\
             \x20   var seen = 0;\n\
             \x20   var amount = {expression};\n\
             \x20   if (1 << amount) != 12345 {{ seen = 0; }}\n\
             \x20   if (1 >> amount) != 12345 {{ seen = 0; }}\n\
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
            "a shift by `{expression}` should compile — the amount is a runtime value:\n{}",
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

/// `docs/character-literals.md` §5 — the third spelling moved no hash.
///
/// `'a'` is the integer 97 and nothing past `int_value` knows which
/// spelling was written, so the two programs below are one program. This
/// is the same claim `ids_are_stable_across_the_operator_set` makes for
/// hexadecimal, checked the way `bitwise.md` §1.1 says it should be:
/// against the other spelling rather than against a number written down.
///
/// Checking it against its sibling rather than against a literal hash is
/// deliberate. A frozen hash here would fail on any future encoder
/// change, including a correct one, and say nothing about the property
/// this slice is responsible for — which is that *these two texts agree*,
/// whatever they agree on.
#[test]
fn a_character_literal_hashes_as_its_integer() {
    let scratch = scratch("ids-character-literal");
    let spellings = [
        ("numbers", "fn f(c: int) -> [] int { return c + 48 + 10; }\n"),
        ("characters", "fn f(c: int) -> [] int { return c + '0' + '\\n'; }\n"),
    ];

    let mut hashes = Vec::new();
    for (name, source) in spellings {
        let path = scratch.join(format!("{name}.ls"));
        std::fs::write(&path, source).expect("a writable fixture");
        let output = Command::new(BIN).arg("ids").arg(&path).output().expect("the compiler runs");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        hashes.push(String::from_utf8(output.stdout).expect("hashes are ascii"));
    }

    assert_eq!(
        hashes[0], hashes[1],
        "`'0'` and `48` should be one node, so these should be one program"
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

    // Past the ceiling the growth loop used to stop at.
    //
    // `read_file` *doubled* from 64 KiB until a read came back strictly
    // shorter than the buffer, because `fs_read` cannot report
    // truncation (`docs/file-handles.md` §1). It stopped after eight
    // attempts, so the largest capacity was 8 MiB and *any* file of
    // 8,388,608 bytes or more was refused — under a comment claiming the
    // limit was 16 MiB, which is why nothing caught it. Raising it to
    // fifteen attempts moved the bound to 1 GiB; reading through a
    // **handle** removed it, because `file_read` says when the file is
    // over and nothing has to guess (`porting.md` §10).
    //
    // 9 MB rather than something just over the line, so this keeps
    // testing a file read in many chunks rather than an off-by-one: it
    // is two doublings past where the old bound was, and 130 reads
    // through the handle.
    //
    // `u64` and not the inferred `i32`: 300_000 * 7919 is 2.4 billion,
    // which overflows an `i32`. The first version of this line did not
    // say so, passed under `cargo test --release` where an overflow
    // wraps, and failed on CI, which runs `cargo test` with the checks
    // on. A test for a language whose whole position is that overflow
    // must not wrap silently is a poor place to let one.
    let past_ceiling: String =
        (0..300_000u64).map(|i| format!("{:029}\n", (i * 7919) % 300_000)).collect();
    assert!(past_ceiling.len() > 8 * 1024 * 1024, "the fixture has to clear the old 8 MiB bound");
    let huge = scratch.join("past-ceiling.txt");
    std::fs::write(&huge, &past_ceiling).expect("a writable fixture");
    let (ours, status) = run(&exe, &[&huge], "");
    assert_eq!(status, Some(0), "a file past the old 8 MiB ceiling should now sort");
    assert_eq!(ours.lines().count(), 300_000, "it lost lines past the old ceiling");
    assert!(
        ours.lines().collect::<Vec<_>>().windows(2).all(|w| w[0] <= w[1]),
        "a file past the old ceiling came out unsorted"
    );

    let _ = std::fs::remove_dir_all(&scratch);
}

/// `benches/three/` — every build still computes the same answer.
///
/// `docs/against-c-and-rust.md` compares lex-sys to C and Rust on the
/// same algorithm, and the comparison means nothing unless the three
/// sources really are the same algorithm. Each prints a checksum; this
/// checks they agree.
///
/// **Not a timing gate.** Wall-clock in CI is noise, and
/// `scripts/three.py` is where the numbers come from. This runs each
/// build once, so a drifted translation is a red build rather than a
/// quietly wrong table.
///
/// The C and Rust halves are skipped when their compilers are not on the
/// path. The lex-sys halves always run.
#[test]
fn the_three_language_benchmarks_agree() {
    let dir = repo_root().join("benches").join("three");
    let scratch = scratch("three");

    /// Build, run once, and hand back what it printed.
    fn checksum(exe: &Path) -> String {
        let run = Command::new(exe).output().expect("the benchmark runs");
        assert_eq!(run.status.code(), Some(0), "`{}` exited badly", exe.display());
        String::from_utf8_lossy(&run.stdout).trim().to_owned()
    }

    let cc = ["cc", "clang", "gcc"].into_iter().find(|c| which(c));
    let have_rustc = which("rustc");

    // (lex-sys source, C source, Rust source, extra C flags)
    let groups: [(&str, &str, &str, &[&str]); 3] = [
        ("mandelbrot.ls", "mandelbrot.c", "mandelbrot.rs", &["-DCHECKED=1"]),
        ("sieve.ls", "sieve.c", "sieve.rs", &[]),
        // Two files per language, because the boundary is the point
        // (`docs/purity.md` §1). The C half is linked below.
        ("purity.ls", "purity.c", "purity.rs", &["-DPROMISED=0"]),
    ];

    for (index, (ours, in_c, in_rust, flags)) in groups.iter().enumerate() {
        let exe = scratch.join(format!("ls{index}"));
        let build = Command::new(BIN)
            .args([
                "build".as_ref(),
                dir.join(ours).as_os_str(),
                "--std".as_ref(),
                "-o".as_ref(),
                exe.as_os_str(),
            ])
            .output()
            .expect("the compiler runs");
        assert!(
            build.status.success(),
            "`{ours}` should compile, but the compiler said:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let expected = checksum(&exe);
        assert!(!expected.is_empty(), "`{ours}` printed no checksum");

        // A benchmark split across a compilation boundary has its other
        // half beside it, named `<stem>_lib.<extension>`.
        let library = |source: &str| -> Option<PathBuf> {
            let (stem, extension) = source.rsplit_once('.').expect("a source file has a suffix");
            let candidate = dir.join(format!("{stem}_lib.{extension}"));
            candidate.exists().then_some(candidate)
        };

        if let Some(cc) = cc {
            let exe = scratch.join(format!("c{index}"));
            let build = Command::new(cc)
                .args(["-O2"])
                .args(*flags)
                .arg(dir.join(in_c))
                .args(library(in_c))
                .arg("-o")
                .arg(&exe)
                .output()
                .expect("the C compiler runs");
            assert!(
                build.status.success(),
                "`{in_c}` should compile:\n{}",
                String::from_utf8_lossy(&build.stderr)
            );
            assert_eq!(checksum(&exe), expected, "`{in_c}` disagrees with `{ours}`");
        }

        if have_rustc {
            let exe = scratch.join(format!("rs{index}"));
            let mut link: Vec<String> = Vec::new();
            if let Some(half) = library(in_rust) {
                let archive = scratch.join(format!("librs{index}.a"));
                assert!(
                    Command::new("rustc")
                        .args(["-O", "--crate-type=staticlib"])
                        .arg(&half)
                        .arg("-o")
                        .arg(&archive)
                        .status()
                        .expect("rustc runs")
                        .success(),
                    "`{}` should compile",
                    half.display()
                );
                link = vec![
                    "-L".into(),
                    scratch.display().to_string(),
                    "-l".into(),
                    format!("static=rs{index}"),
                ];
            }
            let build = Command::new("rustc")
                .args(["-O", "-Coverflow-checks=on"])
                .arg(dir.join(in_rust))
                .args(&link)
                .arg("-o")
                .arg(&exe)
                .output()
                .expect("rustc runs");
            assert!(
                build.status.success(),
                "`{in_rust}` should compile:\n{}",
                String::from_utf8_lossy(&build.stderr)
            );
            assert_eq!(checksum(&exe), expected, "`{in_rust}` disagrees with `{ours}`");
        }
    }

    // The f64 pair has no lex-sys half — that is §4's whole point — so the
    // two of them are checked against each other.
    if let (Some(cc), true) = (cc, have_rustc) {
        let in_c = scratch.join("cf64");
        let in_rust = scratch.join("rsf64");
        assert!(
            Command::new(cc)
                .args(["-O2"])
                .arg(dir.join("mandelbrot_f64.c"))
                .arg("-o")
                .arg(&in_c)
                .status()
                .expect("the C compiler runs")
                .success()
        );
        assert!(
            Command::new("rustc")
                .arg("-O")
                .arg(dir.join("mandelbrot_f64.rs"))
                .arg("-o")
                .arg(&in_rust)
                .status()
                .expect("rustc runs")
                .success()
        );
        assert_eq!(
            checksum(&in_c),
            checksum(&in_rust),
            "the two f64 builds disagree, so §4's precision figure is not what it says"
        );
    }

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Is `name` an executable on `PATH`?
fn which(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
}

/// `docs/purity.md` §2 — the predicate, checked against cases chosen to
/// break it.
///
/// The interesting rows are the ones that are *not* pure despite a `[]`
/// row: `poke` writes through a unique reference, which is `std.vec`'s
/// `set` exactly, and is the whole reason the predicate is two conditions
/// rather than one.
///
/// Each case is a whole program, because pass 2 emits what `main`
/// reaches and nothing else (`standard-library.md` §5.2) — a function
/// nobody calls is not in `Program::funcs` to ask about.
#[test]
fn purity_is_the_row_plus_what_a_reference_may_do() {
    // (program, function name, is it pure)
    let cases: [(&str, &str, bool); 6] = [
        (
            "fn double(x: int) -> [] int { return x + x; }\n\
             fn main(world: World) -> [] int {\n\
             \x20   let Split { io, ffi, fs, heap, args } = split(world);\n\
             \x20   release(args); release(heap); release(fs); release(ffi); release(io);\n\
             \x20   return double(0);\n\
             }\n",
            "double",
            true,
        ),
        // A shared reference is read-only, so reading through one is pure.
        (
            "fn first[&r](s: &r [byte]) -> [] int { return int_of(s[0]); }\n\
             fn main(world: World) -> [] int {\n\
             \x20   let Split { io, ffi, fs, heap, args } = split(world);\n\
             \x20   release(args); release(heap); release(fs); release(ffi); release(io);\n\
             \x20   return first(\"a\") - 97;\n\
             }\n",
            "first",
            true,
        ),
        // A `[]` row and a unique reference: `std.vec`'s `set` exactly.
        (
            "fn poke[&r](s: &!r [byte]) -> [] int { s[0] = byte_of(1); return 0; }\n\
             fn main(world: World) -> [] int {\n\
             \x20   let Split { io, ffi, fs, heap, args } = split(world);\n\
             \x20   release(args); release(heap); release(fs); release(ffi); release(io);\n\
             \x20   var out = 0;\n\
             \x20   region a { let s = alloc_slice[a](2, byte_of(0)); out = poke(s); }\n\
             \x20   return out;\n\
             }\n",
            "poke",
            false,
        ),
        // A unique reference inside a tuple still writes.
        (
            "fn pair[&r](t: (int, &!r [byte])) -> [] int { return t.0; }\n\
             fn main(world: World) -> [] int {\n\
             \x20   let Split { io, ffi, fs, heap, args } = split(world);\n\
             \x20   release(args); release(heap); release(fs); release(ffi); release(io);\n\
             \x20   var out = 0;\n\
             \x20   region a { let s = alloc_slice[a](2, byte_of(0)); out = pair((0, s)); }\n\
             \x20   return out;\n\
             }\n",
            "pair",
            false,
        ),
        // Effects are not pure, however local they look.
        (
            "fn shout[&i](io: &!i Io) -> [io_write] int { putchar(io, 10); return 0; }\n\
             fn main(world: World) -> [] int {\n\
             \x20   let Split { io, ffi, fs, heap, args } = split(world);\n\
             \x20   release(args); release(heap); release(fs); release(ffi);\n\
             \x20   var out = 0;\n\
             \x20   borrow mut io as &!i in { out = shout(i); }\n\
             \x20   release(io);\n\
             \x20   return out;\n\
             }\n",
            "shout",
            false,
        ),
        // A local arena is invisible from outside, so it does not count.
        (
            "fn scratch(n: int) -> [] int {\n\
             \x20   var total = 0;\n\
             \x20   region a { let s = alloc_slice[a](n, byte_of(1)); total = len(s); }\n\
             \x20   return total;\n\
             }\n\
             fn main(world: World) -> [] int {\n\
             \x20   let Split { io, ffi, fs, heap, args } = split(world);\n\
             \x20   release(args); release(heap); release(fs); release(ffi); release(io);\n\
             \x20   return scratch(4) - 4;\n\
             }\n",
            "scratch",
            true,
        ),
    ];

    for (source, name, expected) in cases {
        let ast = lex_sys_syntax::parse(source)
            .unwrap_or_else(|d| panic!("`{name}` should parse: {}", d.message));
        let program = lex_sys_ir::lower(&ast)
            .unwrap_or_else(|d| panic!("`{name}` should check: {}", d.message));
        let id = program.find(name).unwrap_or_else(|| panic!("`{name}` should be emitted"));
        assert_eq!(
            program.func(id).is_pure(),
            expected,
            "`{name}` should {}be pure",
            if expected { "" } else { "not " }
        );
    }
}

/// `docs/floating-point.md` §4 — `truncate` traps exactly where C is
/// undefined.
///
/// NaN, both infinities, and any magnitude at or past `2^63`. C says the
/// behaviour is undefined; this says the process stops, which is §2.1's
/// rule applying where it belongs — the result would be a number, and
/// there is no number it could honestly be.
///
/// Not reject fixtures, for `slicing.md` §8's reason: a trapping program
/// is one that compiled.
#[test]
fn truncate_traps_where_c_is_undefined() {
    let scratch = scratch("float-truncate");
    // (expression, does it trap)
    let cases = [
        ("0.0 / 0.0", true),
        ("1.0 / 0.0", true),
        ("0.0 - 1.0 / 0.0", true),
        ("1.0e30", true),
        ("-1.0e30", true),
        // `2^63` is about 9.223e18, so this is inside and that is not.
        ("9.0e18", false),
        ("1.0e19", true),
        ("0.0", false),
        ("-2.7", false),
    ];

    for (index, (expression, traps)) in cases.iter().enumerate() {
        let source = format!(
            "fn main(world: World) -> [] int {{\n\
             \x20   let Split {{ io, ffi, fs, heap, args }} = split(world);\n\
             \x20   release(args); release(heap); release(fs); release(ffi); release(io);\n\
             \x20   var seen = truncate({expression});\n\
             \x20   if seen == 12345 {{ seen = 0; }}\n\
             \x20   return 0;\n\
             }}\n"
        );
        let path = scratch.join(format!("t{index}.ls"));
        std::fs::write(&path, &source).expect("a writable fixture");
        let exe = scratch.join(format!("t{index}"));
        let build = Command::new(BIN)
            .args(["build".as_ref(), path.as_os_str(), "-o".as_ref(), exe.as_os_str()])
            .output()
            .expect("the compiler runs");
        assert!(
            build.status.success(),
            "`truncate({expression})` should compile — the value is a runtime one:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let run = Command::new(&exe).output().expect("the program runs");
        if *traps {
            assert_eq!(
                run.status.code(),
                None,
                "`truncate({expression})` should be killed by a signal, not exit"
            );
        } else {
            assert_eq!(run.status.code(), Some(0), "`truncate({expression})` should not trap");
        }
    }

    let _ = std::fs::remove_dir_all(&scratch);
}

/// The corpus the printer is checked against, and the driver that prints
/// it (`docs/float-printing.md` §5).
///
/// The expected output *is* the literal that was written. `{:e}` is the
/// shortest decimal that reads back to the same bits, and the lexer's
/// `f64::from_str` is correctly rounded, so a program that prints back
/// what it was given agrees with the oracle by construction -- and one
/// that does not has disagreed about digits, not about notation.
fn float_corpus() -> Vec<f64> {
    let mut values: Vec<f64> = Vec::new();

    // Every normal power of two. These are the values with *uneven*
    // neighbours -- the gap below is half the gap above -- and they are
    // the case a printer gets wrong first, so none of them is sampled.
    for e in 1..=2046u64 {
        values.push(f64::from_bits(e << 52));
    }
    // Every subnormal power of two, down to the smallest float there is,
    // and the all-ones mantissa beside each: the top and the bottom of
    // every subnormal binade.
    for i in 0..52 {
        values.push(f64::from_bits(1u64 << i));
        values.push(f64::from_bits((1u64 << (i + 1)) - 1));
    }
    // Every power of ten in range, where the decimal and the binary
    // grids line up worst.
    for k in -307..=308 {
        values.push(format!("1e{k}").parse().expect("a power of ten in range"));
    }
    // The ones with a reputation.
    for text in [
        "0.1",
        "0.3",
        "0.5",
        "1.0",
        "100.0",
        "1e23",
        "9.999999999999999e22",
        "2.9802322387695312e-8",
        "1.7976931348623157e308",
        "2.2250738585072014e-308",
        "5e-324",
        "3.141592653589793",
        "2.718281828459045",
        "1.1125369292536007e-308",
    ] {
        values.push(text.parse().expect("a float in range"));
    }

    // And a deterministic spread of bit patterns, so the corpus is not
    // only the cases someone thought of. splitmix64 rather than a
    // dependency: the seed is fixed, so a failure here reproduces.
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    while values.len() < 9000 {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        let candidate = f64::from_bits(z);
        if candidate.is_finite() {
            values.push(candidate);
        }
    }
    values
}

const SHOW_LS: &str = "\
module main;

import std.fmt;
import std.io;

fn show[&i](i: &!i Io, x: float) -> [io_write] int {
    region a {
        let buf = alloc_slice[a](24, byte_of(0));
        let n = fmt.float_into(buf, x);
        io.write_all(i, buf[0..n]);
        io.newline(i);
    }
    return 0;
}
";

/// Build and run a program that prints `values`, one per line.
fn print_floats(tag: &str, values: &[f64]) -> Vec<String> {
    let mut source = String::from(SHOW_LS);
    source.push_str("\nfn main(world: World) -> [] int {\n");
    source.push_str("    let Split { io, ffi, fs, heap, args } = split(world);\n");
    source.push_str("    release(args); release(heap); release(fs); release(ffi);\n");
    source.push_str("    borrow mut io as &!i in {\n");
    for value in values {
        // `{:e}` is the shortest round-tripping form, which is both a
        // literal the lexer reads back exactly and the line the program
        // should print.
        source.push_str(&format!("        show(i, {value:e});\n"));
    }
    source.push_str("    }\n    release(io);\n    return 0;\n}\n");

    let dir = scratch(tag);
    let path = dir.join("corpus.ls");
    std::fs::write(&path, &source).expect("a writable fixture");
    let exe = dir.join("corpus");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            "--std".as_ref(),
            path.as_os_str(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(
        build.status.success(),
        "the corpus program should compile:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&exe).output().expect("the program runs");
    assert_eq!(run.status.code(), Some(0), "the corpus program should exit 0");
    let lines: Vec<String> =
        String::from_utf8_lossy(&run.stdout).lines().map(str::to_owned).collect();
    let _ = std::fs::remove_dir_all(&dir);
    lines
}

#[test]
fn shortest_printing_agrees_with_an_oracle() {
    let values = float_corpus();
    let lines = print_floats("float-corpus", &values);
    assert_eq!(lines.len(), values.len(), "one line per value");

    let mut wrong = Vec::new();
    for (value, line) in values.iter().zip(&lines) {
        let expected = format!("{value:e}");
        if &expected != line {
            wrong.push(format!("{expected} printed as {line}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} values printed differently from the oracle; first few:\n{}",
        wrong.len(),
        values.len(),
        wrong.iter().take(10).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// The three values that have no literal, and the signed zero. They are
/// spelled rather than refused (`docs/floating-point.md` §2), and the
/// spellings are the oracle's.
#[test]
fn the_values_with_no_literal_are_spelled() {
    let source = format!(
        "{SHOW_LS}\nfn main(world: World) -> [] int {{\n\
         \x20   let Split {{ io, ffi, fs, heap, args }} = split(world);\n\
         \x20   release(args); release(heap); release(fs); release(ffi);\n\
         \x20   let huge = 1.0e308;\n\
         \x20   let infinite = huge * 10.0;\n\
         \x20   let nothing = 0.0;\n\
         \x20   borrow mut io as &!i in {{\n\
         \x20       show(i, infinite);\n\
         \x20       show(i, -infinite);\n\
         \x20       show(i, infinite - infinite);\n\
         \x20       show(i, nothing / nothing);\n\
         \x20       show(i, -nothing);\n\
         \x20   }}\n\
         \x20   release(io);\n\
         \x20   return 0;\n\
         }}\n"
    );
    let dir = scratch("float-specials");
    let path = dir.join("specials.ls");
    std::fs::write(&path, &source).expect("a writable fixture");
    let exe = dir.join("specials");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            "--std".as_ref(),
            path.as_os_str(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(&exe).output().expect("the program runs");
    let expected = format!(
        "{:e}\n{:e}\n{:e}\n{:e}\n{:e}\n",
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        f64::NAN,
        -0.0f64
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A buffer too short is answered with -1 rather than a trap or a partial
/// line: the caller chose the buffer, so the caller hears about it.
#[test]
fn a_short_buffer_is_refused_rather_than_overrun() {
    let source = "\
module main;

import std.fmt;
import std.io;

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(ffi);
    var code = 0;
    region a {
        let small = alloc_slice[a](4, byte_of(0));
        if fmt.float_into(small, 3.141592653589793) == 0 - 1 {
            code = 7;
        }
        let enough = alloc_slice[a](24, byte_of(0));
        if fmt.float_into(enough, 3.141592653589793) != 19 {
            code = 9;
        }
    }
    release(io);
    return code;
}
";
    let dir = scratch("float-short-buffer");
    let path = dir.join("short.ls");
    std::fs::write(&path, source).expect("a writable fixture");
    let exe = dir.join("short");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            "--std".as_ref(),
            path.as_os_str(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(&exe).output().expect("the program runs");
    assert_eq!(run.status.code(), Some(7), "a short buffer answers -1, a long one the length");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Read `lex-sys authority --output json` for one program.
fn authority_json(source: &str, tag: &str) -> String {
    let dir = scratch(tag);
    let path = dir.join("program.ls");
    std::fs::write(&path, source).expect("a writable fixture");
    let out = Command::new(BIN)
        .args([
            "authority".as_ref(),
            "--std".as_ref(),
            "--output".as_ref(),
            "json".as_ref(),
            path.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let _ = std::fs::remove_dir_all(&dir);
    text
}

fn field(json: &str, name: &str) -> usize {
    let needle = format!("\"{name}\": ");
    let at = json.find(&needle).unwrap_or_else(|| panic!("no `{name}` in:\n{json}"));
    let rest = &json[at + needle.len()..];
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    rest[..end].parse().expect("a number")
}

/// `docs/compile-time.md` §2 and §3 — the pass does what it says.
///
/// Checked through the report rather than through a disassembler, for the
/// reason §9 gives for having a report at all: CI builds on two platforms
/// and `objdump` is not one of the things they share.
#[test]
fn constants_are_evaluated_at_compile_time() {
    let source = "\
fn factorial(n: int) -> [] int {
    if n < 2 { return 1; }
    return n * factorial(n - 1);
}
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(ffi); release(io);
    let arithmetic = 2 + 3 * 4 - 14;
    return factorial(5) - 120 + arithmetic;
}
";
    let json = authority_json(source, "fold-report");
    assert!(field(&json, "folded_operators") > 0, "the operators should fold:\n{json}");
    assert_eq!(field(&json, "folded_calls"), 1, "`factorial(5)` should fold:\n{json}");
}

/// §5 — running out of fuel is not an error, and not visible in the answer.
///
/// `fib(24)` needs about 150 000 calls, which is past the budget, so the
/// call survives into the binary. The program still prints 46368, which
/// is the whole claim: the budget decides how fast the answer arrives and
/// never what it is.
#[test]
fn running_out_of_fuel_leaves_a_working_program() {
    let source = "\
import std.io;
fn fib(n: int) -> [] int {
    if n < 2 { return n; }
    return fib(n - 1) + fib(n - 2);
}
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(ffi);
    borrow mut io as &!i in {
        io.print_int(i, fib(23));
        io.newline(i);
        io.print_int(i, fib(24));
        io.newline(i);
    }
    release(io);
    return 0;
}
";
    let json = authority_json(source, "fold-fuel-report");
    assert_eq!(
        field(&json, "folded_calls"),
        1,
        "`fib(23)` is inside the budget and `fib(24)` is not:\n{json}"
    );

    let dir = scratch("fold-fuel");
    let path = dir.join("fuel.ls");
    std::fs::write(&path, source).expect("a writable fixture");
    let exe = dir.join("fuel");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            "--std".as_ref(),
            path.as_os_str(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(&exe).output().expect("the program runs");
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "28657\n46368\n",
        "the folded call and the one that ran agree"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// §4 — a trap that is merely *possible* is untouched.
///
/// The reject fixtures cover the certain ones. This is the other side:
/// the same operators with a value the compiler cannot know still emit
/// their check and still stop the process, which is the guarantee
/// `defined-behaviour.md` §2.1 makes and this slice must not have
/// weakened.
#[test]
fn a_possible_trap_still_traps() {
    let dir = scratch("fold-possible-trap");
    for (index, (setup, expression)) in
        [("9223372036854775807", "n + 1"), ("0", "1 / n"), ("64", "1 << n")].iter().enumerate()
    {
        let source = format!(
            "fn main(world: World) -> [] int {{\n\
             \x20   let Split {{ io, ffi, fs, heap, args }} = split(world);\n\
             \x20   release(args); release(heap); release(fs); release(ffi); release(io);\n\
             \x20   var n = {setup};\n\
             \x20   return {expression};\n\
             }}\n"
        );
        let path = dir.join(format!("trap{index}.ls"));
        std::fs::write(&path, &source).expect("a writable fixture");
        let exe = dir.join(format!("trap{index}"));
        let build = Command::new(BIN)
            .args(["build".as_ref(), path.as_os_str(), "-o".as_ref(), exe.as_os_str()])
            .output()
            .expect("the compiler runs");
        assert!(
            build.status.success(),
            "`{expression}` should compile — `n` is a runtime value:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let run = Command::new(&exe).output().expect("the program runs");
        assert_eq!(run.status.code(), None, "`{expression}` should be killed by a signal");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// §6 — folding is a fact about the *host* arithmetic agreeing with the
/// target's, and the float row is the one worth checking.
///
/// If the two ever disagreed, a program would print one thing when its
/// arithmetic folded and another when it did not. Both are computed here
/// and compared, which is a tighter check than a table of expected
/// strings: it cannot pass by both sides being wrong in the same way, and
/// it needs no oracle.
#[test]
fn a_folded_float_is_the_same_float() {
    let source = "\
import std.fmt;
import std.io;
fn show[&i](i: &!i Io, x: float) -> [io_write] int {
    region a {
        let buf = alloc_slice[a](24, byte_of(0));
        let n = fmt.float_into(buf, x);
        io.write_all(i, buf[0..n]);
        io.newline(i);
    }
    return 0;
}
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(ffi);
    // The same expression twice: once from literals, which folds, and
    // once through `var`s, which does not.
    var a = 0.1;
    var b = 0.2;
    var c = 3.0;
    borrow mut io as &!i in {
        show(i, 0.1 + 0.2 * 3.0);
        show(i, a + b * c);
        show(i, 1.0 / 3.0);
        var one = 1.0;
        var three = 3.0;
        show(i, one / three);
    }
    release(io);
    return 0;
}
";
    let dir = scratch("fold-float");
    let path = dir.join("float.ls");
    std::fs::write(&path, source).expect("a writable fixture");
    let exe = dir.join("float");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            "--std".as_ref(),
            path.as_os_str(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(&exe).output().expect("the program runs");
    let lines: Vec<&str> = String::from_utf8_lossy(&run.stdout)
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>()
        .leak()
        .iter()
        .map(|s| s.as_str())
        .collect();
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[0], lines[1], "`0.1 + 0.2 * 3.0` folded and unfolded must agree");
    assert_eq!(lines[2], lines[3], "`1.0 / 3.0` folded and unfolded must agree");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------
// The folder against the backend (`docs/differential.md`)
// ---------------------------------------------------------------------

/// The integers every integer operator is tried on: both ends of the
/// range and one step in, the shift amount's edges on both sides, and the
/// small values every rule has a case for. Fourteen, so 196 pairs.
const DIFF_INTS: &[&str] = &[
    "-9223372036854775808",
    "-9223372036854775807",
    "-64",
    "-63",
    "-2",
    "-1",
    "0",
    "1",
    "2",
    "3",
    "63",
    "64",
    "9223372036854775806",
    "9223372036854775807",
];

/// The same for floats: both zeros, a value that is not exact in binary,
/// the largest finite magnitudes, the smallest subnormal and the smallest
/// normal, both infinities, and NaN with each sign.
const DIFF_FLOATS: &[&str] = &[
    "0.0",
    "-0.0",
    "1.0",
    "-1.0",
    "0.1",
    "3.0",
    "1.0e308",
    "-1.0e308",
    "5.0e-324",
    "2.2250738585072014e-308",
    "(1.0 / 0.0)",
    "(-1.0 / 0.0)",
    "(0.0 / 0.0)",
    "(-(0.0 / 0.0))",
];

const DIFF_BOOLS: &[&str] = &["false", "true"];

/// One expression the folder can evaluate: an operator, its operand type,
/// and one or two operands spelled as literals.
struct Case {
    ty: &'static str,
    ret: &'static str,
    op: &'static str,
    a: &'static str,
    b: Option<&'static str>,
}

impl Case {
    fn expr(&self, a: &str, b: &str) -> String {
        match self.op.strip_prefix('u') {
            Some(unary) => format!("{unary}({a})"),
            None => format!("({a}) {} ({b})", self.op),
        }
    }

    fn literal(&self) -> String {
        self.expr(self.a, self.b.unwrap_or(""))
    }

    /// The operator as a function of its operands, which is what the call
    /// arm folds through `evaluate_calls` rather than during lowering.
    fn helper(&self) -> String {
        let name = match self.op {
            "+" => "add",
            "-" => "sub",
            "*" => "mul",
            "/" => "div",
            "%" => "rem",
            "<<" => "shl",
            ">>" => "shr",
            "&" => "band",
            "|" => "bor",
            "^" => "bxor",
            "==" => "eq",
            "!=" => "ne",
            "<" => "lt",
            "<=" => "le",
            ">" => "gt",
            ">=" => "ge",
            "&&" => "land",
            "||" => "lor",
            "u-" => "neg",
            "u~" => "bnot",
            "u!" => "not",
            other => unreachable!("no helper for `{other}`"),
        };
        format!("{}_{name}", self.ty)
    }
}

/// Every result printed as an `int`, so one line format covers all three
/// result types -- and a float as its **bits**, so `-0.0` and `0.0`, and
/// two NaNs, cannot compare equal by accident.
fn shown(ret: &str, e: &str) -> String {
    match ret {
        "int" => e.to_owned(),
        "bool" => format!("b2i({e})"),
        _ => format!("bits_of({e})"),
    }
}

fn differential_cases() -> Vec<Case> {
    let comparison = |op: &str| matches!(op, "==" | "!=" | "<" | "<=" | ">" | ">=" | "&&" | "||");
    let mut cases = Vec::new();
    let mut binary = |ty: &'static str, ops: &[&'static str], values: &[&'static str]| {
        for &op in ops {
            let ret = if comparison(op) { "bool" } else { ty };
            for &a in values {
                for &b in values {
                    cases.push(Case { ty, ret, op, a, b: Some(b) });
                }
            }
        }
    };
    binary(
        "int",
        &["+", "-", "*", "/", "%", "<<", ">>", "&", "|", "^", "==", "!=", "<", "<=", ">", ">="],
        DIFF_INTS,
    );
    binary("float", &["+", "-", "*", "/", "==", "!=", "<", "<=", ">", ">="], DIFF_FLOATS);
    binary("bool", &["==", "!=", "&&", "||"], DIFF_BOOLS);
    for (ty, op, values) in
        [("int", "u-", DIFF_INTS), ("int", "u~", DIFF_INTS), ("float", "u-", DIFF_FLOATS)]
            .into_iter()
            .chain([("bool", "u!", DIFF_BOOLS)])
    {
        for &a in values {
            cases.push(Case { ty, ret: ty, op, a, b: None });
        }
    }
    cases
}

/// Parse `k v` lines, the one format both programs print.
fn numbered(text: &str) -> Vec<(usize, i64)> {
    text.lines()
        .map(|line| {
            let (k, v) = line.split_once(' ').expect("a `k v` line");
            (k.parse().expect("a case number"), v.parse().expect("a value"))
        })
        .collect()
}

const DIFF_SHARED: &str = "\
import std.io;
fn b2i(b: bool) -> [] int { if b { return 1; } return 0; }
";

/// C2(b) of the audit, and the test `fold.rs` had been citing for months
/// without it existing: **every operator the folder evaluates, on every
/// pair of boundary operands, gives the same answer folded as the
/// compiled program gives at run time** -- the same value, or a trap on
/// both sides.
///
/// Three arms per case, because the folder has two entrances:
///
/// - **literal**: `(a) op (b)` in a function of its own, folded during
///   lowering. A trap here is a `constant-traps` refusal, and `check`
///   reports every one, so one run of `check` is the folder's whole trap
///   set.
/// - **call**: `op_fn(a, b)`, a pure function on literal arguments,
///   folded by `evaluate_calls` after lowering. `authority` counts the
///   calls it folded, which is how this test knows the arm did not
///   silently fall through to run time.
/// - **run time**: the same operator on operands chosen by a loop counter
///   the compiler cannot see, so neither the folder nor Cranelift has a
///   constant to work with. A trap kills the process, so it reports on
///   standard error -- unbuffered, where standard output would lose
///   everything since the last flush -- and is restarted past the case
///   that killed it.
///
/// Measured against deliberately broken folders before it was trusted
/// (`differential.md` §3): each of five one-line mutations was caught.
#[test]
fn the_folder_agrees_with_the_backend() {
    let cases = differential_cases();
    let dir = scratch("differential");

    // ---- the folder's trap set: one function per case, on line k + 1 ----
    let literal: Vec<String> = cases
        .iter()
        .enumerate()
        .map(|(k, c)| format!("fn l{k}() -> [] {} {{ return {}; }}", c.ret, c.literal()))
        .collect();
    let probe = dir.join("literal.ls");
    std::fs::write(
        &probe,
        format!(
            "{}\nfn main(world: World) -> [] int {{ release(world); return 0; }}\n",
            literal.join("\n")
        ),
    )
    .expect("a writable fixture");
    let check = Command::new(BIN)
        .args(["check".as_ref(), probe.as_os_str(), "--output".as_ref(), "json".as_ref()])
        .output()
        .expect("the compiler runs");
    let report = String::from_utf8_lossy(&check.stdout);
    let refusals = report.matches("\"rule\":").count();
    assert_eq!(
        refusals,
        report.matches("\"rule\": \"constant-traps\"").count(),
        "a literal case was refused for something other than trapping:\n{report}"
    );
    let folder_traps: std::collections::BTreeSet<usize> = report
        .match_indices("\"line\": ")
        .map(|(at, key)| {
            let digits: String =
                report[at + key.len()..].chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<usize>().expect("a line number") - 1
        })
        .collect();
    assert_eq!(folder_traps.len(), refusals, "one refusal per trapping case");

    // ---- the two folded arms, for every case that has a value ----
    let mut helpers = std::collections::BTreeMap::new();
    for c in &cases {
        let body = match c.op.strip_prefix('u') {
            Some(unary) => {
                format!("fn {}(a: {}) -> [] {} {{ return {unary}a; }}", c.helper(), c.ty, c.ret)
            }
            None => format!(
                "fn {}(a: {ty}, b: {ty}) -> [] {} {{ return a {} b; }}",
                c.helper(),
                c.ret,
                c.op,
                ty = c.ty
            ),
        };
        helpers.insert(c.helper(), body);
    }
    let mut folded = String::from(DIFF_SHARED);
    folded.push_str(
        "fn out[&i](i: &!i Io, k: int, v: int) -> [io_write] int {\n    \
         io.print_int(i, k); io.space(i); io.print_int(i, v); io.newline(i); return 0;\n}\n",
    );
    for body in helpers.values() {
        folded.push_str(body);
        folded.push('\n');
    }
    let mut calls = String::new();
    for (k, c) in cases.iter().enumerate().filter(|(k, _)| !folder_traps.contains(k)) {
        let args = match c.b {
            Some(b) => format!("{}, {b}", c.a),
            None => c.a.to_owned(),
        };
        folded.push_str(&literal[k]);
        folded.push('\n');
        folded.push_str(&format!(
            "fn c{k}() -> [] {} {{ return {}({args}); }}\n",
            c.ret,
            c.helper()
        ));
        calls.push_str(&format!(
            "        out(i, {k}, {}); out(i, {k}, {});\n",
            shown(c.ret, &format!("l{k}()")),
            shown(c.ret, &format!("c{k}()"))
        ));
    }
    folded.push_str(&format!(
        "fn main(world: World) -> [] int {{\n    \
         let Split {{ io, ffi, fs, heap, args }} = split(world);\n    \
         release(args); release(heap); release(fs); release(ffi);\n    \
         borrow mut io as &!i in {{\n{calls}    }}\n    release(io);\n    return 0;\n}}\n"
    ));
    let folded_path = dir.join("folded.ls");
    std::fs::write(&folded_path, folded).expect("a writable fixture");

    let valued = cases.len() - folder_traps.len();
    let authority = Command::new(BIN)
        .args([
            "authority".as_ref(),
            folded_path.as_os_str(),
            "--std".as_ref(),
            "--output".as_ref(),
            "json".as_ref(),
        ])
        .output()
        .expect("the compiler runs");
    let authority = String::from_utf8_lossy(&authority.stdout);
    assert!(
        authority.contains(&format!("\"folded_calls\": {valued},")),
        "every call arm must fold, or the arm is run time against run time:\n{authority}"
    );

    let exe = dir.join("folded");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            "--std".as_ref(),
            folded_path.as_os_str(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(&exe).output().expect("the program runs");
    assert!(run.status.success(), "nothing in the folded program can trap");
    let mut at_compile_time = std::collections::BTreeMap::new();
    for (k, v) in numbered(&String::from_utf8_lossy(&run.stdout)) {
        if let Some(seen) = at_compile_time.insert(k, v) {
            assert_eq!(
                seen,
                v,
                "the two halves of the folder disagree on `{}` ({}): lowering says {seen}, \
                 `evaluate_calls` says {v}",
                cases[k].literal(),
                cases[k].ty
            );
        }
    }
    assert_eq!(at_compile_time.len(), valued);

    // ---- run time: operands the compiler cannot see ----
    let mut runtime = String::from(DIFF_SHARED);
    runtime.push_str(
        "\
fn digits[&o](buf: &!o [byte], at: int, n: int) -> [] int {
    var p = at;
    var m = n;
    if m > 0 { m = -m; }
    if m == 0 { p = p - 1; buf[p] = byte_of('0'); }
    while m != 0 {
        p = p - 1;
        buf[p] = byte_of('0' - m % 10);
        m = m / 10;
    }
    if n < 0 { p = p - 1; buf[p] = byte_of('-'); }
    return p;
}
fn out[&i](i: &!i Io, k: int, v: int) -> [err_write] int {
    region r {
        let buf = alloc_slice[r](48, byte_of(0));
        buf[47] = byte_of('\\n');
        var p = digits(buf, 47, v);
        p = p - 1;
        buf[p] = byte_of(' ');
        p = digits(buf, p, k);
        io.error_all(i, buf[p..48]);
    }
    return 0;
}
fn first[&a](a: &a Args) -> [args] int {
    let s = arg(a, 1);
    var n = 0;
    var p = 0;
    while p < len(s) { n = n * 10 + (int_of(s[p]) - '0'); p = p + 1; }
    return n;
}
",
    );
    for (name, ty, values) in
        [("ival", "int", DIFF_INTS), ("fval", "float", DIFF_FLOATS), ("bval", "bool", DIFF_BOOLS)]
    {
        runtime.push_str(&format!("fn {name}(k: int) -> [] {ty} {{\n"));
        for (i, v) in values.iter().enumerate() {
            runtime.push_str(&format!("    if k == {i} {{ return {v}; }}\n"));
        }
        runtime.push_str(&format!("    return {};\n}}\n", values[0]));
    }
    // One section per operator, in the order `differential_cases` made
    // them, decoding the case number back into two operand indices.
    runtime.push_str("fn case(k: int) -> [] int {\n");
    let mut start = 0;
    while start < cases.len() {
        let c = &cases[start];
        let count = cases[start..].iter().take_while(|d| d.ty == c.ty && d.op == c.op).count();
        let (getter, n) = match c.ty {
            "int" => ("ival", DIFF_INTS.len()),
            "float" => ("fval", DIFF_FLOATS.len()),
            _ => ("bval", DIFF_BOOLS.len()),
        };
        let e = match c.b {
            None => c.expr(&format!("{getter}(j)"), ""),
            Some(_) => c.expr(&format!("{getter}(j / {n})"), &format!("{getter}(j % {n})")),
        };
        runtime.push_str(&format!(
            "    if k < {} {{\n        let j = k - {start};\n        return {};\n    }}\n",
            start + count,
            shown(c.ret, &e)
        ));
        start += count;
    }
    runtime.push_str(&format!(
        "    return 0;\n}}\n\
         fn main(world: World) -> [] int {{\n    \
         let Split {{ io, ffi, fs, heap, args }} = split(world);\n    \
         release(heap); release(fs); release(ffi);\n    \
         var k = 0;\n    \
         borrow args as &a in {{ k = first(a); }}\n    \
         release(args);\n    \
         borrow mut io as &!i in {{\n        \
         while k < {} {{ out(i, k, case(k)); k = k + 1; }}\n    }}\n    \
         release(io);\n    return 0;\n}}\n",
        cases.len()
    ));
    let runtime_path = dir.join("runtime.ls");
    std::fs::write(&runtime_path, runtime).expect("a writable fixture");
    let exe = dir.join("runtime");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            "--std".as_ref(),
            runtime_path.as_os_str(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    // Every trap is a process the kernel kills, and each one costs
    // whatever the host does with a crash -- a core dump, or a handler
    // `core_pattern` pipes it to. So the loop is bounded three ways and
    // says what it saw when it stops: a run that neither finishes nor
    // traps is killed, a run of traps the folder did not predict stops
    // early rather than paying for one crash per remaining case, and the
    // whole phase has a budget.
    let started = std::time::Instant::now();
    let mut at_run_time = std::collections::BTreeMap::new();
    let mut runtime_traps = std::collections::BTreeSet::new();
    let mut unpredicted = Vec::new();
    let mut runs = 0;
    let mut slowest = (std::time::Duration::ZERO, 0, false);
    let mut trap_time = std::time::Duration::ZERO;
    let report = |what: &str,
                  runs: usize,
                  traps: usize,
                  slowest: (std::time::Duration, usize, bool),
                  trap_time: std::time::Duration| {
        let pattern = std::fs::read_to_string("/proc/sys/kernel/core_pattern")
            .map(|p| p.trim().to_owned())
            .unwrap_or_else(|_| "unreadable".to_owned());
        format!(
            "{what}: {runs} runs, {traps} traps, {:.1?} elapsed; slowest run {:.1?} \
             (from case {}, {}); {:.1?} per trapping run; core_pattern `{pattern}`",
            started.elapsed(),
            slowest.0,
            slowest.1,
            if slowest.2 { "trapped" } else { "finished" },
            trap_time / u32::try_from(traps.max(1)).unwrap_or(1),
        )
    };
    let mut next = 0;
    while next < cases.len() {
        let began = std::time::Instant::now();
        let mut child = Command::new(&exe)
            .arg(next.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("the program runs");
        let mut pipe = child.stderr.take().expect("a piped standard error");
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = std::io::Read::read_to_string(&mut pipe, &mut text);
            text
        });
        let status = loop {
            if let Some(status) = child.try_wait().expect("the program can be waited on") {
                break status;
            }
            if began.elapsed() > std::time::Duration::from_secs(30) {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "{}",
                    report(
                        &format!("a run from case {next} neither finished nor trapped in 30 s"),
                        runs + 1,
                        runtime_traps.len(),
                        slowest,
                        trap_time
                    )
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        let stderr = reader.join().expect("the reader thread finishes");
        runs += 1;
        let took = began.elapsed();
        let mut reached = next;
        for (k, v) in numbered(&stderr) {
            at_run_time.insert(k, v);
            reached = k + 1;
        }
        if took > slowest.0 {
            slowest = (took, next, !status.success());
        }
        if status.success() {
            assert_eq!(reached, cases.len(), "a clean exit must have run every case");
            break;
        }
        // A trap is a signal, never an exit code (`defined-behaviour.md`
        // §1): an exit status here would be a different failure.
        assert_eq!(status.code(), None, "case {reached} ended without a signal");
        trap_time += took;
        runtime_traps.insert(reached);
        if !folder_traps.contains(&reached) {
            unpredicted.push(format!("`{}` ({})", cases[reached].literal(), cases[reached].ty));
            assert!(
                unpredicted.len() <= 20,
                "{}; traps the folder did not predict, first 20:\n{}",
                report("stopped early", runs, runtime_traps.len(), slowest, trap_time),
                unpredicted.join("\n")
            );
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(300),
            "{}",
            report("over the 300 s budget", runs, runtime_traps.len(), slowest, trap_time)
        );
        next = reached + 1;
    }

    // ---- the comparison ----
    let mut disagreements = Vec::new();
    for (k, c) in cases.iter().enumerate() {
        let what = format!("`{}` ({})", c.literal(), c.ty);
        match (folder_traps.contains(&k), runtime_traps.contains(&k)) {
            (true, false) => disagreements.push(format!(
                "{what}: the folder refuses it as a trap, the program answers {}",
                at_run_time[&k]
            )),
            (false, true) => disagreements.push(format!(
                "{what}: the folder answers {}, the program traps",
                at_compile_time[&k]
            )),
            (false, false) if at_compile_time[&k] != at_run_time[&k] => disagreements.push(
                format!("{what}: folded {}, at run time {}", at_compile_time[&k], at_run_time[&k]),
            ),
            _ => {}
        }
    }
    assert!(
        disagreements.is_empty(),
        "{} of {} cases disagree:\n{}",
        disagreements.len(),
        cases.len(),
        disagreements.join("\n")
    );
    // The sizes `differential.md` §3 reports, so a change to the operand
    // tables or the operator set is a change to the document too.
    assert_eq!((cases.len(), folder_traps.len()), (5156, 436));
    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/differential.md` §4 — the one value that was not the same on
/// every target.
///
/// IEEE-754 leaves the sign and payload of a *generated* NaN to the
/// hardware, and x86-64 sets the sign where aarch64 does not, so
/// `bits_of(0.0 / 0.0)` printed `-2251799813685248` on one CI runner and
/// `9221120237041090560` on the other. Every way this language can make
/// a NaN is tried here, at run time and folded, and all of them must read
/// back as the one pattern.
#[test]
fn every_nan_has_one_bit_pattern() {
    let source = "\
import std.io;
fn show[&i](i: &!i Io, x: float) -> [io_write] int {
    io.print_int(i, bits_of(x));
    io.newline(i);
    return 0;
}
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(ffi);
    var zero = 0.0;
    var one = 1.0;
    var inf = 1.0 / 0.0;
    borrow mut io as &!i in {
        show(i, 0.0 / 0.0);
        show(i, -(0.0 / 0.0));
        show(i, zero / zero);
        show(i, -(zero / zero));
        show(i, inf - inf);
        show(i, inf * zero);
        show(i, sqrt(-one));
        show(i, (zero / zero) + one);
        show(i, -((zero / zero) * one));
    }
    release(io);
    return 0;
}
";
    let dir = scratch("one-nan");
    let path = dir.join("nan.ls");
    std::fs::write(&path, source).expect("a writable fixture");
    let exe = dir.join("nan");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            "--std".as_ref(),
            path.as_os_str(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(&exe).output().expect("the program runs");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 9);
    for line in lines {
        assert_eq!(line, "9221120237041090560", "every NaN reads as 0x7ff8000000000000");
    }

    // And the reason the canonicalisation exists, measured on the machine
    // running this test rather than asserted: the hardware's own NaN.
    // Where it already is the canonical pattern, `bits_of` changes
    // nothing; where it is not, the program above would have printed the
    // other one.
    let hardware = (std::hint::black_box(0.0_f64) / std::hint::black_box(0.0_f64)).to_bits();
    if cfg!(target_arch = "x86_64") {
        assert_eq!(hardware, 0xfff8_0000_0000_0000, "x86-64's indefinite NaN has its sign set");
    }
    if cfg!(target_arch = "aarch64") {
        assert_eq!(hardware, 0x7ff8_0000_0000_0000, "aarch64's default NaN is positive");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/compile-time-data.md` §1.1 — the row that is a capability
/// argument rather than a convenience.
///
/// An arena is one 64 KiB chunk and exhausting it traps, so a
/// 65 536-entry `[int]` table — 512 KB, the shape a CRC or a 16-bit codec
/// uses — cannot be built in a `region` at all. A program that released
/// `heap` therefore cannot have one. A `static` has no such ceiling,
/// because the data is in the file rather than in a chunk.
///
/// Both halves are checked here, because the claim is a comparison: the
/// `region` version must trap and the `static` version must work.
#[test]
fn a_static_outgrows_what_an_arena_could_hold() {
    let body = "\
    var i = 0;\n\
    while i < 65536 {\n\
        table[i] = i * 3;\n\
        i = i + 1;\n\
    }\n";

    let with_static = format!(
        "fn main(world: World) -> [] int {{\n\
         \x20   let Split {{ io, ffi, fs, heap, args }} = split(world);\n\
         \x20   release(args); release(heap); release(fs); release(ffi); release(io);\n\
         \x20   if len(big) != 65536 {{ return 1; }}\n\
         \x20   return big[65535] - 196605;\n\
         }}\n\
         static big: [int] {{\n\
         \x20   let table = alloc_slice[static](65536, 0);\n\
         {body}\
         \x20   return table;\n\
         }}\n"
    );
    let with_region = format!(
        "fn main(world: World) -> [] int {{\n\
         \x20   let Split {{ io, ffi, fs, heap, args }} = split(world);\n\
         \x20   release(args); release(heap); release(fs); release(ffi); release(io);\n\
         \x20   region a {{\n\
         \x20       let table = alloc_slice[a](65536, 0);\n\
         {body}\
         \x20       if table[65535] != 196605 {{ return 1; }}\n\
         \x20   }}\n\
         \x20   return 0;\n\
         }}\n"
    );

    let dir = scratch("static-big");
    for (name, source, should_run) in
        [("static", with_static, true), ("region", with_region, false)]
    {
        let path = dir.join(format!("{name}.ls"));
        std::fs::write(&path, &source).expect("a writable fixture");
        let exe = dir.join(name);
        let build = Command::new(BIN)
            .args(["build".as_ref(), path.as_os_str(), "-o".as_ref(), exe.as_os_str()])
            .output()
            .expect("the compiler runs");
        assert!(
            build.status.success(),
            "`{name}` should compile — the arena's limit is a run-time one:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let run = Command::new(&exe).output().expect("the program runs");
        if should_run {
            assert_eq!(
                run.status.code(),
                Some(0),
                "a 512 KB `static` is data in the binary, so there is no chunk to exhaust"
            );
        } else {
            assert_eq!(
                run.status.code(),
                None,
                "512 KB in a 64 KiB arena traps, which is the whole of §1.1's second row"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// §2 — a `static` is read-only data, and the program never builds it.
///
/// Checked through the authority report rather than a disassembler, for
/// the reason `compile-time.md` §9 gives: CI builds on two platforms and
/// `objdump` is not among the things they share. A program whose only
/// arithmetic is inside a `static` performs nothing and folds nothing at
/// run time, which is what "the loop ran in the compiler" looks like from
/// outside.
#[test]
fn a_static_needs_no_authority_and_no_heap() {
    let source = "\
static table: [int] {
    let t = alloc_slice[static](8, 0);
    var i = 0;
    while i < 8 { t[i] = i * i; i = i + 1; }
    return t;
}
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(ffi); release(io);
    return table[3] - 9;
}
";
    let json = authority_json(source, "static-authority");
    assert!(json.contains("\"effects\": []"), "a `static` performs nothing:\n{json}");
    assert!(json.contains("\"foreign_symbols\": []"), "and reaches no foreign code:\n{json}");

    let dir = scratch("static-runs");
    let path = dir.join("static.ls");
    std::fs::write(&path, source).expect("a writable fixture");
    let exe = dir.join("static");
    let build = Command::new(BIN)
        .args(["build".as_ref(), path.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(&exe).output().expect("the program runs");
    assert_eq!(run.status.code(), Some(0), "`table[3]` is 9, computed during compilation");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/layout.md` §4 — the report agrees with what the backend emits.
///
/// The check that matters is `stride`, because that is the number a
/// program can *observe*: an arena is a fixed 64 KiB and exhausting it
/// traps, so how many elements fit is exactly `65536 / stride`. A report
/// that drifted from the emitter would disagree with where the trap
/// lands, and this finds it by walking the boundary from both sides.
#[test]
fn the_layout_report_says_what_a_type_costs() {
    let dir = scratch("layout-report");
    let source = "\
struct Rgb { r: byte, g: byte, b: byte }
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(ffi); release(io);
    region a {
        let s = alloc_slice[a](COUNT, Rgb { r: byte_of(1), g: byte_of(2), b: byte_of(3) });
        if len(s) == 0 { return 1; }
    }
    return 0;
}
";
    let path = dir.join("rgb.ls");
    std::fs::write(&path, source.replace("COUNT", "1")).expect("a writable fixture");

    let report = Command::new(BIN)
        .args(["layout".as_ref(), path.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(report.status.success(), "{}", String::from_utf8_lossy(&report.stderr));
    let text = String::from_utf8_lossy(&report.stdout);

    let row = text
        .lines()
        .find(|l| l.starts_with("Rgb"))
        .unwrap_or_else(|| panic!("no `Rgb` row in:\n{text}"));
    let columns: Vec<u32> =
        row.split_whitespace().skip(1).map(|n| n.parse().expect("a number")).collect();
    assert_eq!(columns[0], 3, "three leaves:\n{text}");
    assert_eq!(columns[1], 24, "eight bytes each today:\n{text}");
    assert_eq!(columns[2], 3, "one byte each packed — §2's whole point:\n{text}");
    let stride = columns[3];
    assert_eq!(stride, 24, "and the stride is what a traversal pays:\n{text}");

    // Now the observable half: an arena is 64 KiB, so `65536 / stride`
    // elements fit and one more does not.
    let fits = 65536 / stride;
    for (count, should_run) in [(fits, true), (fits + 1, false)] {
        let path = dir.join(format!("rgb{count}.ls"));
        std::fs::write(&path, source.replace("COUNT", &count.to_string()))
            .expect("a writable fixture");
        let exe = dir.join(format!("rgb{count}"));
        let build = Command::new(BIN)
            .args(["build".as_ref(), path.as_os_str(), "-o".as_ref(), exe.as_os_str()])
            .output()
            .expect("the compiler runs");
        assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
        let run = Command::new(&exe).output().expect("the program runs");
        if should_run {
            assert_eq!(
                run.status.code(),
                Some(0),
                "{count} × {stride} bytes is exactly one arena, so it fits"
            );
        } else {
            assert_eq!(
                run.status.code(),
                None,
                "one element past the arena traps, which is how the stride is observable"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/benchmarks-game.md` §2 — every ported program prints the answer
/// the Benchmarks Game publishes.
///
/// The published value is the point. Two programs I wrote agreeing with
/// each other proves only that I made the same mistake twice; agreeing
/// with a number someone else published is evidence. Each is run at the
/// size the Game states an answer for, which is small enough to be a
/// test rather than a benchmark — `scripts/game.py` runs the same
/// binaries at the sizes that take seconds.
#[test]
fn benchmark_game_programs_print_the_published_answer() {
    let root = repo_root().join("benches").join("game");
    let cases: [(&str, &str, &str); 3] = [
        ("fannkuch", "7", "228\nPfannkuchen(7) = 16\n"),
        ("spectral", "100", "1.274219991\n"),
        (
            "binarytrees",
            "10",
            "stretch tree of depth 11\t check: 4095\n\
             1024\t trees of depth 4\t check: 31744\n\
             256\t trees of depth 6\t check: 32512\n\
             64\t trees of depth 8\t check: 32704\n\
             16\t trees of depth 10\t check: 32752\n\
             long lived tree of depth 10\t check: 2047\n",
        ),
    ];

    let dir = scratch("benchmark-game");
    for (name, size, expected) in cases {
        let exe = dir.join(name);
        let build = Command::new(BIN)
            .args([
                "build".as_ref(),
                "--std".as_ref(),
                root.join(format!("{name}.ls")).as_os_str(),
                "-o".as_ref(),
                exe.as_os_str(),
            ])
            .output()
            .expect("the compiler runs");
        assert!(
            build.status.success(),
            "`{name}` should compile:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let run = Command::new(&exe).arg(size).output().expect("the program runs");
        assert_eq!(run.status.code(), Some(0), "`{name}` should exit 0");
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            expected,
            "`{name}` at N={size} should print what the Benchmarks Game publishes"
        );

        // And the C counterpart, which is only a fair comparison if it
        // computes the same thing (§2's rule).
        let c_exe = dir.join(format!("{name}_c"));
        let cc = Command::new("cc")
            .args([
                "-O2".as_ref(),
                root.join(format!("{name}.c")).as_os_str(),
                "-o".as_ref(),
                c_exe.as_os_str(),
            ])
            .output()
            .expect("a C compiler");
        assert!(cc.status.success(), "{}", String::from_utf8_lossy(&cc.stderr));
        let c_run = Command::new(&c_exe).arg(size).output().expect("the C program runs");
        assert_eq!(
            String::from_utf8_lossy(&c_run.stdout),
            expected,
            "`{name}.c` must compute the same thing, or the timing means nothing"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/bulk-io.md` §3.2 — a faster program is not a more powerful one.
///
/// This is the whole argument of the slice, so it is pinned byte for
/// byte rather than field by field: the same program written with
/// `putchar` and with `write_bytes` must produce an *identical*
/// authority report. If a future change gave the bulk primitive its own
/// effect label, its own capability, or a `foreign_symbols` entry for
/// the `fwrite` it lowers to, this fails — and it should, because §2's
/// complaint was precisely that the fast path used to cost more
/// authority than the slow one.
///
/// `foreign_symbols` staying empty is the subtle half. `write_bytes`
/// does call libc, but the program neither declared that call nor can
/// choose it; it is the builtin's implementation, the same way `putchar`
/// has always been libc's. A report that named `fwrite` here would be
/// telling a reader to audit something they cannot influence.
#[test]
fn bulk_output_costs_exactly_what_one_byte_costs() {
    const PROLOGUE: &str = "\
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(ffi);
";
    let per_byte = format!(
        "{PROLOGUE}    borrow mut io as &!i in {{ putchar(i, 104); putchar(i, 105); }}\n    release(io);\n    return 0;\n}}\n"
    );
    let bulk = format!(
        "{PROLOGUE}    borrow mut io as &!i in {{ write_bytes(i, \"hi\"); }}\n    release(io);\n    return 0;\n}}\n"
    );

    let slow = authority_json(&per_byte, "bulk-authority-putchar");
    let fast = authority_json(&bulk, "bulk-authority-write");
    assert_eq!(slow, fast, "`write_bytes` must report exactly what `putchar` reports (§3.2)");
    assert!(slow.contains("\"effects\": [\"io_write\"]"), "and that is `io_write`:\n{slow}");
    assert!(
        slow.contains("\"foreign_symbols\": []"),
        "a builtin's own libc call is not the program reaching foreign code:\n{slow}"
    );
}

/// `docs/file-handles.md` §4.1 — the prefix survives the rewrite.
///
/// `bulk-io.md` §3.2's rule is that a program must not look more
/// powerful for having been written better, and the test there pins the
/// authority report **byte for byte**: `write_bytes` writes the same
/// stream `putchar` writes, so a new label would have been a regression.
///
/// Here the rule needs one more word, and the difference is real rather
/// than a loophole. A handle program *does* perform something the path
/// program does not — it holds a descriptor across statements — and §4
/// chose to say so, in a label carrying **no argument**. So the test is
/// not byte-identity but the thing §3.2 was actually protecting:
///
///   * every label that carries an argument is identical, so the
///     directory is still named and named the same way;
///   * the handle program adds exactly one label, and it has no
///     argument to widen.
///
/// If `read` ever grew a path — §4's first option, which would make a
/// handle unusable by a function that was not told where it came from —
/// the first assertion catches it.
#[test]
fn a_handle_reports_the_same_prefix_a_path_does() {
    let prologue = "\
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(ffi); release(io);
";
    let whole_file = format!(
        "{prologue}    region a {{\n        \
         var buffer = alloc_slice[a](8, byte_of(0));\n        \
         borrow fs as &c in {{ fs_read(c, \"/tmp/lex-sys-authority.txt\", buffer); }}\n    \
         }}\n    release(fs);\n    return 0;\n}}\n"
    );
    let handle = format!(
        "{prologue}    region a {{\n        \
         var buffer = alloc_slice[a](8, byte_of(0));\n        \
         borrow fs as &c in {{\n            \
         match open_read(c, \"/tmp/lex-sys-authority.txt\") {{\n                \
         Opened::Ok(f) => {{\n                    var file = f;\n                    \
         borrow mut file as &!h in {{ file_read(h, buffer); }}\n                    \
         file_close(file);\n                }}\n                \
         Opened::Failed(e) => {{ }}\n            }}\n        }}\n    }}\n    \
         release(fs);\n    return 0;\n}}\n"
    );

    let by_path = authority_json(&whole_file, "handle-authority-path");
    let by_handle = authority_json(&handle, "handle-authority-handle");

    // The labels that name something. Every label has an `"argument"` key;
    // a path-free one spells it `null`, which is exactly the difference
    // this test is about.
    let arguments = |json: &str| {
        json.match_indices("{ \"name\":")
            .filter_map(|(at, _)| json[at..].find('}').map(|end| json[at..at + end + 1].to_owned()))
            .filter(|row| !row.contains("\"argument\": null"))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        arguments(&by_path),
        arguments(&by_handle),
        "a handle must name the same directory a path does (§4.1):\n{by_path}\n{by_handle}"
    );
    assert!(
        by_path.contains("\"fs_read\""),
        "the path program should name the directory at all:\n{by_path}"
    );

    // And the one label the rewrite adds carries nothing to widen.
    assert!(
        by_handle.contains("\"file_read\""),
        "a handle program performs `file_read` (§4.1):\n{by_handle}"
    );
    assert!(
        by_handle.contains("{ \"name\": \"file_read\", \"argument\": null, \"bounded\": true }"),
        "`file_read` must carry no argument, or a handle would need its own path:\n{by_handle}"
    );
    assert!(!by_path.contains("file_read"), "the path program never opens a handle:\n{by_path}");
}

/// §3 — `write_bytes` goes through the same stream `putchar` does.
///
/// The reason the primitive lowers to `fwrite` on `stdout` rather than
/// POSIX `write` on descriptor 1: `putchar` is buffered by stdio, so a
/// raw descriptor write would have jumped the queue and the two kinds of
/// output would interleave in the wrong order. Nothing about the types
/// catches that — only running it does. The fixture writes a strictly
/// increasing sequence through alternating primitives, so any reordering
/// shows up as an out-of-order digit rather than as a subtle diff.
#[test]
fn bulk_and_per_byte_output_share_one_stream() {
    let dir = scratch("bulk-ordering");
    let path = dir.join("ordering.ls");
    let source = "\
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(ffi);
    borrow mut io as &!i in {
        var round = 0;
        while round < 2000 {
            putchar(i, 48);
            write_bytes(i, \"12\");
            putchar(i, 51);
            write_bytes(i, \"456\");
            putchar(i, 55);
            write_bytes(i, \"89\");
            round = round + 1;
        }
    }
    release(io);
    return 0;
}
";
    std::fs::write(&path, source).expect("a writable fixture");
    let exe = dir.join("ordering");
    let build = Command::new(BIN)
        .args(["build".as_ref(), path.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(&exe).output().expect("the program runs");
    assert_eq!(run.status.code(), Some(0), "it exits cleanly");
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "0123456789".repeat(2000),
        "the two primitives must interleave in program order, not in stream order"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/utf8.md` §3 — the decoder agrees with an independent oracle.
///
/// The fixture next door pins ten named cases; this pins the *rule* over
/// inputs nobody chose, the way `shortest_printing_agrees_with_an_oracle`
/// does for floats. Rust's own `String::from_utf8_lossy` implements the
/// same maximal-subpart rule §3.2 adopts, and `str::from_utf8` the same
/// strict validity §3.1 adopts, so both columns have a reference that is
/// not this repository.
///
/// §2 is why the oracle is not GNU `wc -m`: it counts `f5 80 80 80` as
/// one character, and that sequence encodes a value above U+10FFFF.
#[test]
fn utf8_decoding_agrees_with_an_oracle() {
    let dir = scratch("utf8-oracle");
    let program = dir.join("oracle.ls");
    std::fs::write(
        &program,
        "\
import std.io;
import std.utf8;

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(ffi);
    borrow mut io as &!i in {
        region a {
            var buf = alloc_slice[a](4096, byte_of(0));
            var more = true;
            while more {
                var n = 0;
                var k = 0;
                var eof = false;
                while k < 4 {
                    let c = getchar(i);
                    if c < 0 { eof = true; k = 4; }
                    else { n = n | (c << (8 * k)); k = k + 1; }
                }
                if eof { more = false; }
                else {
                    var j = 0;
                    while j < n { buf[j] = byte_of(getchar(i)); j = j + 1; }
                    let text = buf[0..n];
                    io.print_int(i, utf8.count(text));
                    io.write_all(i, \" \");
                    if utf8.is_valid(text) { io.write_all(i, \"1\"); }
                    else { io.write_all(i, \"0\"); }
                    io.newline(i);
                }
            }
        }
    }
    release(io);
    return 0;
}
",
    )
    .expect("a writable fixture");

    let exe = dir.join("oracle");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            program.as_os_str(),
            "--std".as_ref(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    // Deterministic inputs across four shapes: well-formed text at every
    // width, raw noise, valid text with bytes corrupted, and valid text
    // cut short. The last two are where a decoder's skip rule shows.
    let mut seed: u64 = 0x5eed_1234_9abc_def0;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let sample = "héllo wörld 日本語 😀🎉 αβγ";
    let mut cases: Vec<Vec<u8>> = Vec::new();
    for i in 0..2000u32 {
        let mut c: Vec<u8> = Vec::new();
        match i % 4 {
            0 => {
                for _ in 0..(next() % 12 + 1) {
                    let p = match next() % 4 {
                        0 => next() % 0x80,
                        1 => 0x80 + next() % (0x800 - 0x80),
                        2 => 0x800 + next() % (0x1_0000 - 0x800),
                        _ => 0x1_0000 + next() % (0x11_0000 - 0x1_0000),
                    } as u32;
                    if let Some(ch) = char::from_u32(p) {
                        let mut b = [0u8; 4];
                        c.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
                    }
                }
            }
            1 => {
                for _ in 0..(next() % 16 + 1) {
                    c.push((next() % 256) as u8);
                }
            }
            2 => {
                c.extend_from_slice(sample.as_bytes());
                for _ in 0..(next() % 3 + 1) {
                    let at = (next() as usize) % c.len();
                    c[at] = (next() % 256) as u8;
                }
            }
            _ => {
                let b = sample.as_bytes();
                let take = (next() as usize) % b.len() + 1;
                c.extend_from_slice(&b[..take]);
            }
        }
        if !c.is_empty() {
            cases.push(c);
        }
    }

    let mut input: Vec<u8> = Vec::new();
    let mut expected = String::new();
    for c in &cases {
        input.extend_from_slice(&(c.len() as u32).to_le_bytes());
        input.extend_from_slice(c);
        // The oracle: Rust's own decoder, for both columns.
        let count = String::from_utf8_lossy(c).chars().count();
        let valid = u8::from(std::str::from_utf8(c).is_ok());
        expected.push_str(&format!("{count} {valid}\n"));
    }

    let mut child = Command::new(&exe)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("the program runs");
    {
        use std::io::Write;
        child.stdin.take().expect("stdin").write_all(&input).expect("the write lands");
    }
    let out = child.wait_with_output().expect("it finishes");
    let got = String::from_utf8_lossy(&out.stdout);

    let mismatch = got
        .lines()
        .zip(expected.lines())
        .enumerate()
        .find(|(_, (g, e))| g != e)
        .map(|(i, (g, e))| (i, g.to_owned(), e.to_owned()));
    assert!(
        mismatch.is_none(),
        "case {:?} disagrees with Rust: got {:?}, expected {:?} (bytes {:02x?})",
        mismatch.as_ref().map(|m| m.0),
        mismatch.as_ref().map(|m| m.1.clone()),
        mismatch.as_ref().map(|m| m.2.clone()),
        mismatch.as_ref().map(|m| cases[m.0].clone()),
    );
    assert_eq!(got.lines().count(), cases.len(), "every case should have answered");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `examples/cut/` — the third port, checked against GNU `cut`.
///
/// It exists as a probe rather than a demonstration: `utf8.md` §1 said
/// the rest of a string library is "code, not design", and the way to
/// find out *which* code is to write a program that needs it. This one
/// asked for `bytes.count_byte` and `bytes.field`, which is how they
/// arrived — the same route `vec.set` and `vec.swap` took.
///
/// The field specs below are the ones where a hand-rolled splitter goes
/// wrong: empty leading and trailing fields, a line with no delimiter at
/// all (which GNU passes through whole without `-s`), and a field past
/// the end.
#[test]
fn cut_agrees_with_gnu_cut() {
    let dir = scratch("example-cut");
    let exe = dir.join("cut");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            repo_root().join("examples/cut/cut.ls").as_os_str(),
            "--std".as_ref(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(
        build.status.success(),
        "`cut` should compile, but the compiler said:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let data = "alpha,beta,gamma,delta,epsilon\n\
                one,two,three\n\
                a,b,c,d,e,f,g\n\
                ,leading,empty\n\
                trailing,empty,\n\
                nodelimiterhere\n\
                x,y\n";

    /// Feed `input` on stdin and hand back stdout.
    fn run(cmd: &std::path::Path, args: &[&str], input: &str) -> String {
        use std::io::Write;
        let mut child = Command::new(cmd)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("the program runs");
        let text = input.to_owned();
        let mut stdin = child.stdin.take().expect("stdin");
        std::thread::spawn(move || {
            let _ = stdin.write_all(text.as_bytes());
        });
        let out = child.wait_with_output().expect("it finishes");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    // Same rule as the `base64` and `sort` tests: BSD's `cut` is a
    // different program, so the reference only counts when it is GNU's.
    let reference = std::path::Path::new("/usr/bin/cut");
    let have_reference = Command::new(reference)
        .arg("--version")
        .output()
        .map(|v| v.status.success() && String::from_utf8_lossy(&v.stdout).contains("GNU coreutils"))
        .unwrap_or(false);

    for spec in ["-f1", "-f2", "-f2,4", "-f1-3", "-f3-", "-f2-4", "-f1,3-5", "-f9"] {
        let ours = run(&exe, &["-d,", spec], data);
        // Shape checks, which run with or without a reference: every
        // input line produces exactly one output line.
        assert_eq!(
            ours.lines().count(),
            data.lines().count(),
            "`{spec}` should answer one line per input line"
        );
        if have_reference {
            let theirs = run(reference, &["-d,", spec], data);
            assert_eq!(ours, theirs, "`cut -d, {spec}` differs from GNU cut");
        }
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// The failing half of `cut`, against the same reference as the passing
/// half (`docs/standard-error.md` §1.3).
///
/// This replaces an assertion that said `Some(2)` — the status this
/// program happened to have, taken from a source comment claiming it was
/// "the exit status GNU uses", which it was not. The reference was five
/// lines away the whole time and was asked about eight valid specs and
/// never about the invalid one, because a failing run produced nothing
/// to compare. It does now.
#[test]
fn cut_reports_a_bad_field_list_like_gnu_cut() {
    let dir = scratch("example-cut-bad");
    let exe = dir.join("cut");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            repo_root().join("examples/cut/cut.ls").as_os_str(),
            "--std".as_ref(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let ours = run_without_locale(&exe, &["-d,", "-fzzz"], "");

    // Unconditional: whatever the reference says, a diagnostic belongs on
    // standard error and nothing belongs on standard output.
    assert!(ours.stdout.is_empty(), "a refused `-f` list should print no data");
    assert_eq!(
        String::from_utf8_lossy(&ours.stderr),
        "cut: invalid field value 'zzz'\n",
        "a refused `-f` list should say so on standard error"
    );

    let reference = std::path::Path::new("/usr/bin/cut");
    let have_reference = Command::new(reference)
        .arg("--version")
        .output()
        .map(|v| v.status.success() && String::from_utf8_lossy(&v.stdout).contains("GNU coreutils"))
        .unwrap_or(false);
    if have_reference {
        let theirs = run_without_locale(reference, &["-d,", "-fzzz"], "");
        assert_eq!(ours.status.code(), theirs.status.code(), "GNU cut exits differently");
        // Two differences that are not wording, so the comparison is of
        // what follows the program's own name on the first line.
        //
        // GNU adds a second line pointing at `--help`, which this
        // program does not have. And GNU takes its prefix from `argv[0]`,
        // so invoking it by absolute path makes it call itself
        // `/usr/bin/cut` — where this program has a literal.
        // `docs/standard-error.md` §8 is the open question that is, and
        // this is the measurement under it.
        assert_eq!(
            complaint(&ours.stderr),
            complaint(&theirs.stderr),
            "GNU cut words it differently"
        );
    } else {
        assert_eq!(ours.status.code(), Some(1), "a malformed `-f` list should exit 1");
    }

    // The other refusal: an argument this program does not understand.
    // GNU names the option and this does not, so only the status and the
    // stream are comparable — which is still two things that were
    // neither compared nor comparable before (§1.3).
    let unknown = run_without_locale(&exe, &["--nope"], "");
    assert!(unknown.stdout.is_empty(), "an unrecognised argument should print no data");
    assert_eq!(
        String::from_utf8_lossy(&unknown.stderr),
        "cut: usage: cut -d<c> -f<list>\n",
        "an unrecognised argument should say so on standard error"
    );
    if have_reference {
        let theirs = run_without_locale(reference, &["--nope"], "");
        assert_eq!(unknown.status.code(), theirs.status.code(), "GNU cut exits differently");
    } else {
        assert_eq!(unknown.status.code(), Some(1));
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/standard-error.md` §1.2, which is the measurement the whole
/// design turns on.
///
/// Standard output is fully buffered when it is not a terminal, and a
/// trap does not flush it — so a program that says what is wrong and
/// then dies says nothing at all. Measured before this slice at **zero
/// bytes**, into a file and through a pipe both.
///
/// C guarantees `stderr` is not fully buffered, so the same message on
/// the other stream has already left. That is a property of the stream
/// this compiler picked rather than of anything lex-sys does, which is
/// exactly why it is worth a test: the day the backend reaches for
/// POSIX `write`, or a different stream, this is what notices.
#[test]
fn a_diagnostic_survives_a_trap() {
    let dir = scratch("stderr-trap");
    let source = dir.join("trap.ls");
    std::fs::write(
        &source,
        "fn main(world: World) -> [] int {\n\
        \x20   let Split { io, ffi, fs, heap, args } = split(world);\n\
        \x20   release(ffi); release(fs); release(heap); release(args);\n\
        \x20   var bad = 0;\n\
        \x20   borrow mut io as &!i in {\n\
        \x20       write_bytes(i, \"on stdout\\n\");\n\
        \x20       write_err(i, \"on stderr\\n\");\n\
        \x20       // `byte_of` traps outside 0..255 (`strings.md` §2).\n\
        \x20       bad = int_of(byte_of(300));\n\
        \x20   }\n\
        \x20   release(io);\n\
        \x20   return bad;\n\
        }\n",
    )
    .expect("the fixture is written");

    let exe = dir.join("trap");
    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("it runs");
    assert!(!run.status.success(), "the fixture is supposed to trap");
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        "on stderr\n",
        "the diagnostic did not survive the trap"
    );
    // The other half, and the reason the first half matters: the same
    // message on the output stream is gone.
    assert!(
        run.stdout.is_empty(),
        "standard output flushed on a trap, which would make §1.2's argument moot: {:?}",
        String::from_utf8_lossy(&run.stdout)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/standard-error.md` §4: the negative half of the authority report
/// must not lie about a program whose entire output is a diagnostic.
///
/// A new label with no entry in the report's table produces exactly one
/// wrong sentence — "never touches the console" — about a program that
/// touches nothing else. `authority.md` §2.2 is why that is the half
/// worth guarding: an absent label is a proof, and a proof of the wrong
/// thing is worse than no report.
#[test]
fn a_diagnostic_only_program_touches_the_console() {
    let dir = scratch("stderr-authority");
    let source = dir.join("complain.ls");
    std::fs::write(
        &source,
        "fn shout[&i](io: &!i Io) -> [err_write] int {\n\
        \x20   return write_err(io, \"only a diagnostic\\n\");\n\
        }\n\
        \n\
        fn main(world: World) -> [] int {\n\
        \x20   let Split { io, ffi, fs, heap, args } = split(world);\n\
        \x20   release(ffi); release(fs); release(heap); release(args);\n\
        \x20   borrow mut io as &!i in { shout(i); }\n\
        \x20   release(io);\n\
        \x20   return 1;\n\
        }\n",
    )
    .expect("the fixture is written");

    let report =
        Command::new(BIN).arg("authority").arg(&source).output().expect("the compiler runs");
    assert!(report.status.success(), "{}", String::from_utf8_lossy(&report.stderr));
    let text = String::from_utf8_lossy(&report.stdout);

    assert!(text.contains("err_write"), "the report should name the label:\n{text}");
    assert!(
        !text.contains("the console"),
        "a program whose whole output is a diagnostic touches the console:\n{text}"
    );
    // The rest of the negative half still holds, so the entry narrowed
    // the claim rather than removing it.
    for absent in ["the filesystem", "the heap", "the command line", "foreign code"] {
        assert!(text.contains(absent), "the report should still say `{absent}`:\n{text}");
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// Build one of the `examples/` programs into a scratch directory.
/// `docs/flags.md` §2 — the nine shapes, each read back as itself.
///
/// The fixture is the driver: `tests/accept/flags.ls` prints what it
/// was handed, so §2's table and this test are the same claim, and a
/// fixture that drifted from the document would fail here rather than
/// sit in the tree agreeing with nothing.
///
/// `-d,` and `-d ,` produce the same line on purpose. That is the point
/// of §3's protocol — the program asked for a value, so both spellings
/// of giving one resolve to the same thing, and the caller never learns
/// which was written.
#[test]
fn every_argument_shape() {
    let (dir, exe) = build_example("flags-shapes", "tests/accept/flags.ls", "flags");

    let cases: &[(&[&str], &str)] = &[
        (&["-x"], "short x"),
        (&["-xy"], "short x\nshort y"),
        (&["-d,"], "short d=,"),
        (&["-d", ","], "short d=,"),
        (&["--decode"], "long decode"),
        (&["--delimiter=,"], "long delimiter=,"),
        (&["--delimiter", ","], "long delimiter=,"),
        (&["--", "-x"], "operand -x"),
        (&["-"], "operand -"),
        (&["file.csv"], "operand file.csv"),
        // The value runs out: an empty slice, which is the one case §3
        // says a caller has to test.
        (&["-d"], "short d="),
        // A flag after `--` is an operand, and so is a second `--`.
        (&["--", "--", "-x"], "operand --\noperand -x"),
    ];

    for (args, expected) in cases {
        let run = Command::new(&exe).args(*args).output().expect("the program runs");
        assert_eq!(run.status.code(), Some(0), "`{}` should exit 0", args.join(" "));
        assert_eq!(
            String::from_utf8_lossy(&run.stdout).trim_end(),
            *expected,
            "`{}` should read back as itself",
            args.join(" ")
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/flags.md` §1 — every spelling GNU accepts, on both ports.
///
/// The gap this closes is not a missing feature. Before `std.flags`, six
/// of these rows disagreed with GNU: `cut` refused four of them loudly,
/// and `base64` answered two of them by **encoding its input and exiting
/// 0**, because `--decode` is not two bytes so the test for `-d` was
/// false and the flag was dropped.
///
/// And every base64 test in this file passed exactly `-d`. Twelve input
/// sizes, both directions and three malformed inputs, all through one
/// spelling of one flag — `line-reading.md` §1's shape again, where the
/// dimension the suite never varied was the length of a line.
///
/// # The expectation is written down, and the reference is a second
/// opinion
///
/// The table carries GNU's answer rather than reading it off whatever
/// `/usr/bin/cut` happens to be, because **macOS ships BSD**: BSD `cut`
/// has no `--delimiter` at all, so comparing against it on darwin tests
/// this program against the wrong specification. The base64 test below
/// already says this about empty input and BSD's extra newline; this is
/// the same fact costing a red build before it was applied here.
///
/// So: the written answer is the assertion, on every target. Where the
/// reference *is* GNU — detected by `--version`, which BSD's does not
/// have — it is checked against the same table, so a wrong expectation
/// fails on linux rather than being believed everywhere.
#[test]
fn both_ports_match_gnu_on_every_spelling() {
    let data = b"a,b,c\n";
    let encoded = b"aGk=\n";

    // `(program, args, stdout, exit, whether GNU agrees)`.
    //
    // The `false` rows are the divergences `flags.md` §4 keeps on
    // purpose. They are asserted to still *disagree*, so one that
    // quietly starts agreeing gets moved rather than forgotten.
    let cases: &[(&str, &[&str], &str, i32, bool)] = &[
        ("cut", &["-d,", "-f2"], "b\n", 0, true),
        ("cut", &["-d", ",", "-f2"], "b\n", 0, true),
        ("cut", &["--delimiter=,", "--fields=2"], "b\n", 0, true),
        ("cut", &["--delimiter", ",", "--fields", "2"], "b\n", 0, true),
        ("cut", &["-d,", "-f", "2"], "b\n", 0, true),
        ("cut", &["-f2", "-d,"], "b\n", 0, true),
        ("cut", &["-d,", "-f1,3"], "a,c\n", 0, true),
        ("cut", &["-d,", "-f2", "--"], "b\n", 0, true),
        ("cut", &["-x"], "", 1, true),
        ("cut", &["-d,,", "-f2"], "", 1, true),
        // §4: no abbreviation, because resolving one needs the option
        // table this design does not have.
        ("cut", &["--delim=,", "-f2"], "", 1, false),
        ("base64", &["-d"], "hi", 0, true),
        ("base64", &["--decode"], "hi", 0, true),
        ("base64", &[], "YUdrPQo=\n", 0, true),
        ("base64", &["--"], "YUdrPQo=\n", 0, true),
        ("base64", &["-q"], "", 1, true),
        // §4: `-i` and `-w` are refused rather than accepted and
        // ignored. Neither is implementable here, and accepting one
        // would be §1's silent lie in a second place.
        ("base64", &["-di"], "", 1, false),
        ("base64", &["-w0"], "", 1, false),
    ];

    let (cut_dir, cut_exe) = build_example("flags-cut", "examples/cut/cut.ls", "cut");
    let (b64_dir, b64_exe) = build_example("flags-base64", "examples/base64/base64.ls", "base64");

    fn feed(command: &Path, args: &[&str], input: &[u8]) -> (String, Option<i32>) {
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
            let _ = stdin.write_all(&owned);
            drop(stdin);
        });
        let out = child.wait_with_output().expect("it exits");
        writer.join().expect("the writer thread finishes");
        (String::from_utf8_lossy(&out.stdout).into_owned(), out.status.code())
    }

    /// Whether this reference is GNU coreutils rather than BSD.
    ///
    /// `--version` is the probe because BSD's `cut` does not have it and
    /// GNU's does — the same distinction the table exists for.
    fn is_gnu(path: &Path) -> bool {
        path.exists()
            && Command::new(path)
                .arg("--version")
                .output()
                .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains("GNU coreutils"))
    }

    for (program, args, stdout, exit, agrees) in cases {
        let (exe, input): (&Path, &[u8]) = match *program {
            "cut" => (&cut_exe, data),
            _ => (&b64_exe, encoded),
        };
        let written = ((*stdout).to_owned(), Some(*exit));

        assert_eq!(
            feed(exe, args, input),
            written,
            "`{program} {}` should answer what `flags.md` §1 says it does",
            args.join(" ")
        );

        let reference = PathBuf::from(format!("/usr/bin/{program}"));
        if !is_gnu(&reference) {
            continue;
        }
        let theirs = feed(&reference, args, input);
        if *agrees {
            assert_eq!(
                theirs,
                written,
                "GNU `{program} {}` should answer this too — if it does not, \
                 the expectation in this table is wrong rather than the port",
                args.join(" ")
            );
        } else {
            assert_ne!(
                theirs,
                written,
                "`{program} {}` is a documented divergence (`flags.md` §4); \
                 if it now agrees, move the row rather than deleting it",
                args.join(" ")
            );
        }
    }

    let _ = std::fs::remove_dir_all(&cut_dir);
    let _ = std::fs::remove_dir_all(&b64_dir);
}

/// `docs/first-page.md` §5 — every local documentation link resolves.
///
/// This ran by hand at the end of every slice until now, which is the
/// wrong place for it: a link that rots between one slice and the next
/// is found by whoever clicks it rather than by the build. 150-odd
/// links across `README.md`, `AGENTS.md` and `docs/` is more than a
/// reader should be asked to trust.
///
/// Fenced blocks and inline code are stripped first, because
/// `alloc_slice[a](5, 0)` is `](` followed by a path that is not one.
#[test]
fn every_documentation_link_resolves() {
    let root = repo_root();
    let mut files: Vec<PathBuf> = vec![root.join("README.md"), root.join("AGENTS.md")];
    for entry in std::fs::read_dir(root.join("docs")).expect("docs/ is readable") {
        let path = entry.expect("a readable entry").path();
        if path.extension().is_some_and(|e| e == "md") {
            files.push(path);
        }
    }
    files.sort();

    let mut checked = 0;
    let mut broken: Vec<String> = Vec::new();
    for file in &files {
        let source = std::fs::read_to_string(file).expect("a readable document");
        // Strip fenced blocks, then inline code, then look for `](target)`.
        let mut text = String::new();
        let mut fenced = false;
        for line in source.lines() {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                continue;
            }
            if !fenced {
                text.push_str(line);
                text.push('\n');
            }
        }
        let mut plain = String::new();
        let mut in_code = false;
        for c in text.chars() {
            if c == '`' {
                in_code = !in_code;
                continue;
            }
            if !in_code {
                plain.push(c);
            }
        }

        let directory = file.parent().expect("a parent directory");
        let mut rest = plain.as_str();
        while let Some(at) = rest.find("](") {
            rest = &rest[at + 2..];
            let Some(close) = rest.find(')') else { break };
            let target = &rest[..close];
            rest = &rest[close + 1..];
            if target.starts_with("http") || target.starts_with("mailto") || target.starts_with('#')
            {
                continue;
            }
            // A `#section` suffix names a heading, not a file.
            let path = target.split('#').next().unwrap_or(target);
            if path.is_empty() || path.contains(char::is_whitespace) {
                continue;
            }
            checked += 1;
            if !directory.join(path).exists() {
                broken.push(format!("{} -> {target}", file.display()));
            }
        }
    }

    assert!(checked > 100, "only {checked} links found; the scan is broken, not the links");
    assert!(
        broken.is_empty(),
        "{} broken documentation link(s):\n{}",
        broken.len(),
        broken.join("\n")
    );
}

/// `docs/first-page.md` §5 — the commands the first page shows still exist.
///
/// `AGENTS.md`'s code blocks are fixtures, for the reason §1 there gives:
/// a page that teaches something the compiler no longer does is worse
/// than no page. The same argument reaches the README, whose
/// "compiler's whole surface" block is the first thing a reader tries.
///
/// The check is against `--help`, not against running each one, because
/// what rots here is a subcommand that was renamed or added — which is
/// exactly what this caught when it was written: `layout` and
/// `agent-guidelines` were missing from the block, and `check` had lost
/// its `--output json`.
#[test]
fn the_readme_commands_still_work() {
    let readme = std::fs::read_to_string(repo_root().join("README.md")).expect("a readable README");
    let help = Command::new(BIN).arg("--help").output().expect("the compiler runs");
    let help = String::from_utf8_lossy(&help.stdout).into_owned();

    // Every `lex-sys <word>` the README shows in its surface block.
    let block = readme
        .split("### The compiler's whole surface")
        .nth(1)
        .expect("the README should still document the surface");
    let shown: Vec<&str> = block
        .lines()
        .take_while(|line| !line.starts_with("###"))
        .filter_map(|line| line.trim().strip_prefix("lex-sys "))
        .filter_map(|rest| rest.split_whitespace().next())
        .filter(|word| !word.starts_with('-'))
        .collect();
    assert!(shown.len() >= 6, "the surface block should list the subcommands, found {shown:?}");

    for command in &shown {
        assert!(
            help.contains(&format!("lex-sys {command}")),
            "the README shows `lex-sys {command}` and `--help` does not:\n{help}"
        );
    }

    // And the other way: a subcommand the compiler has and the page does
    // not is the half that goes unnoticed, because nothing breaks.
    //
    // Scoped to the `usage:` block, because `--help` opens with a prose
    // line that also begins "lex-sys" and names no subcommand.
    let usage = help
        .split("usage:")
        .nth(1)
        .expect("`--help` should have a usage block")
        .split("\noptions:")
        .next()
        .expect("a usage block");
    for line in usage.lines() {
        let Some(rest) = line.trim().strip_prefix("lex-sys ") else { continue };
        let Some(word) = rest.split_whitespace().next() else { continue };
        if word.starts_with('-') {
            continue;
        }
        assert!(
            shown.contains(&word),
            "`lex-sys {word}` exists and the README's surface block does not show it"
        );
    }
}

/// `docs/net.md` §1 and §5 — the only network program here is inbound.
///
/// `reach.md` §5.1 frames a `Net` capability as *"a host is a thing
/// worth narrowing to"*, which is the **outbound** question. The one
/// network program in this repository binds a port and waits: it
/// declares `bind`, `listen` and `accept` and never `connect`.
///
/// That is the count §5 rests on — inbound 1, outbound 0 — and the
/// reason `net.md` ends at *settled, not built*: the half that would
/// unblock the `lex-os` join has no asker.
///
/// So this test exists to **fail** when one arrives. A program that
/// connects makes the count wrong, and whoever writes it rewrites §5
/// rather than leaving a document that quietly aged.
#[test]
fn the_only_network_program_is_inbound() {
    let root = repo_root();
    let mut inbound = Vec::new();
    let mut outbound = Vec::new();

    let mut sources: Vec<PathBuf> = Vec::new();
    for directory in ["examples", "std", "tests/accept"] {
        let mut stack = vec![root.join(directory)];
        while let Some(at) = stack.pop() {
            for entry in std::fs::read_dir(&at).expect("a readable directory") {
                let path = entry.expect("a readable entry").path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "ls") {
                    sources.push(path);
                }
            }
        }
    }

    for path in &sources {
        let text = std::fs::read_to_string(path).expect("a readable program");
        for line in text.lines() {
            let Some(rest) = line.trim().strip_prefix("extern fn ") else { continue };
            let Some(name) = rest.split(['[', '(']).next() else { continue };
            let name = name.trim();
            if ["bind", "listen", "accept"].contains(&name) {
                inbound.push(format!("{}:{name}", path.display()));
            }
            if ["connect", "sendto", "getaddrinfo"].contains(&name) {
                outbound.push(format!("{}:{name}", path.display()));
            }
        }
    }

    assert!(!inbound.is_empty(), "`examples/serve/` should still declare the inbound three");
    assert!(
        outbound.is_empty(),
        "an outbound network program has arrived: {outbound:?}\n\
         `net.md` §5 counts zero of them, and that count is the reason the \
         document ends at \"settled, not built\". Rewrite §5 rather than \
         deleting this test."
    );
}

/// Read `authority --output json` for a program, as parsed fields.
///
/// Returns `(effect names, foreign symbols, labels as name=argument)`.
fn authority_of(relative: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
    let out = Command::new(BIN)
        .args([
            "authority".as_ref(),
            repo_root().join(relative).as_os_str(),
            "--std".as_ref(),
            "--output".as_ref(),
            "json".as_ref(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8(out.stdout).expect("the report is utf-8");

    // A small reader rather than a JSON dependency: this crate has none,
    // and the three fields below are flat arrays of strings and objects.
    fn array(text: &str, key: &str) -> Vec<String> {
        let Some(at) = text.find(&format!("\"{key}\"")) else { return Vec::new() };
        let rest = &text[at..];
        let Some(open) = rest.find('[') else { return Vec::new() };
        let Some(close) = rest[open..].find(']') else { return Vec::new() };
        let body = &rest[open + 1..open + close];
        body.split(',')
            .filter_map(|piece| {
                let piece = piece.trim();
                let start = piece.find('"')?;
                let end = piece[start + 1..].find('"')? + start + 1;
                Some(piece[start + 1..end].to_owned())
            })
            .collect()
    }

    // `labels` is objects, so read the pairs out of the same slice.
    let mut labels = Vec::new();
    if let Some(at) = text.find("\"labels\"") {
        let rest = &text[at..];
        if let (Some(open), Some(close)) = (rest.find('['), rest.find(']')) {
            for row in rest[open + 1..close].split('}') {
                let Some(name_at) = row.find("\"name\":") else { continue };
                let name: String = row[name_at + 7..]
                    .trim_start()
                    .trim_start_matches('"')
                    .chars()
                    .take_while(|c| *c != '"')
                    .collect();
                let argument = row.find("\"argument\":").map(|a| {
                    let tail = row[a + 11..].trim_start();
                    if tail.starts_with("null") {
                        String::new()
                    } else {
                        tail.trim_start_matches('"').chars().take_while(|c| *c != '"').collect()
                    }
                });
                match argument.as_deref() {
                    Some("") | None => labels.push(name),
                    Some(value) => labels.push(format!("{name}={value}")),
                }
            }
        }
    }

    (array(&text, "effects"), array(&text, "foreign_symbols"), labels)
}

/// `docs/under-a-grant.md` §2 — a network program reports no network.
///
/// `examples/serve/` binds a TCP port, listens, accepts a connection and
/// answers HTTP. Its effect row is `args` and `ffi`, because sockets are
/// libc and libc is not an authority domain (`reach.md` §5).
///
/// This is pinned rather than merely written down because it is a **gap**,
/// and a gap that nothing observes is one that closes silently. When
/// `reach.md` §6's `Net(host)` row lands, this test fails, and the person
/// who lands it writes the new row here — which is the moment the
/// `lex-os` join in `ROADMAP.md` becomes possible.
#[test]
fn a_network_program_reports_no_network() {
    let (effects, symbols, _) = authority_of("examples/serve/serve.ls");
    assert_eq!(
        effects,
        vec!["args", "ffi"],
        "a program that runs a network server should still report only these \
         two — if it now reports a network label, `under-a-grant.md` §2 and §5 \
         are out of date and so is the roadmap's lex-os row"
    );
    // The symbol list is what covers the difference today, and §3 is why
    // that is a heuristic rather than a wall.
    for expected in ["socket", "bind", "listen", "accept"] {
        assert!(
            symbols.contains(&expected.to_owned()),
            "`{expected}` should be reachable and listed"
        );
    }
}

/// `docs/under-a-grant.md` §5.1 — the report fails closed.
///
/// §3 found that the foreign symbol list is a proof about names and a
/// heuristic about domains, and left the report saying `ffi` as calmly as
/// it says `heap`. A supervisor that reads the row and trusts it would
/// then admit `examples/serve/` under `network: None`.
///
/// So the report says it first, in a field a naive consumer cannot miss:
/// `bounded` is the first key, and it is `false` whenever any reachable
/// label fails to name its own domain -- which is exactly `ffi`. Refusing
/// on `bounded: false` is the safe default; trusting a particular
/// unbounded program anyway is a decision about that program, and it is
/// the supervisor's to make rather than the report's.
///
/// Both directions, because a flag that is always `false` would pass the
/// first half of this test and be worthless.
#[test]
fn the_report_fails_closed() {
    let report = |relative: &str| -> String {
        let out = Command::new(BIN)
            .args(["authority".as_ref(), repo_root().join(relative).as_os_str()])
            .args(["--std", "--output", "json"])
            .output()
            .expect("the compiler runs");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).expect("the report is utf-8")
    };

    let serve = report("examples/serve/serve.ls");
    assert!(
        serve.trim_start().starts_with("{\n  \"bounded\": false,"),
        "a program reaching foreign code must lead with `bounded: false`:\n{serve}"
    );

    let cut = report("examples/cut/cut.ls");
    assert!(
        cut.trim_start().starts_with("{\n  \"bounded\": true,"),
        "a program with no foreign code is bounded, or the flag means nothing:\n{cut}"
    );

    // And the prose form says so before anything else.
    let prose = Command::new(BIN)
        .args(["authority".as_ref(), repo_root().join("examples/serve/serve.ls").as_os_str()])
        .arg("--std")
        .output()
        .expect("the compiler runs");
    let prose = String::from_utf8_lossy(&prose.stdout);
    assert!(prose.starts_with("UNBOUNDED"), "the prose report should open with it:\n{prose}");
    assert!(prose.contains("ffi(\"libc\")    <- unbounded"), "and mark the label:\n{prose}");
}

/// `docs/under-a-grant.md` §3 — the symbol list is a proof about *names*.
///
/// `reach.md` §5.2 said a supervisor reading `socket`, `bind`, `listen`
/// and `accept` knows what it is being asked to run "without trusting a
/// word the program says about itself". The first half is right. The
/// second is not: every foreign call needs a declaration, and the
/// *declaration* is the program's to name.
///
/// Six lines open a socket and report `["syscall"]`. Not a hole to be
/// plugged — refusing this one name would be theatre, since the next
/// spelling is a wrapper. The hole is `Ffi(lib)`'s width (§4).
#[test]
fn the_symbol_list_is_a_proof_about_names() {
    let dir = scratch("under-a-grant");
    let source = dir.join("opaque.ls");
    std::fs::write(
        &source,
        "extern fn syscall[&f](ffi: &f Ffi(\"libc\"), n: int, a: int, b: int, c: int)\n\
             -> [ffi(\"libc\")] int;\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(io); release(fs); release(heap); release(args);\n\
             var r = 0;\n\
             let libc = narrow(ffi, \"libc\");\n\
             borrow libc as &f in { r = syscall(f, 41, 2, 1, 0); }\n\
             release(libc);\n\
             if r < 0 { return 1; }\n\
             return 0;\n\
         }\n",
    )
    .expect("a writable fixture");

    let out = Command::new(BIN)
        .args(["authority".as_ref(), source.as_os_str(), "--output".as_ref(), "json".as_ref()])
        .output()
        .expect("the compiler runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let report = String::from_utf8_lossy(&out.stdout);

    assert!(
        report.contains("\"effects\": [\"ffi\"]"),
        "a program that opens a socket through `syscall` reports only `ffi`:\n{report}"
    );
    assert!(
        report.contains("\"foreign_symbols\": [\"syscall\"]"),
        "and its symbol list names nothing a supervisor could map to a domain:\n{report}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/under-a-grant.md` §2 and §4 — the dimension that *does* work.
///
/// `filesystem.md` §2 took files out of libc and made them builtins under
/// `Fs(p)`, so the path survives into the report. That is the whole reason
/// the grant's filesystem dimension is enforceable while its network and
/// exec dimensions are not — someone already did, for files, what §5 asks
/// for sockets.
#[test]
fn the_filesystem_dimension_is_enforceable() {
    let (_, _, labels) = authority_of("tests/accept/fs_narrowed.ls");
    assert!(
        labels.iter().any(|label| label.starts_with("fs_write=/")),
        "the report should carry the path prefix, not just the dimension: {labels:?}"
    );
}

fn build_example(tag: &str, relative: &str, binary: &str) -> (PathBuf, PathBuf) {
    let dir = scratch(tag);
    let exe = dir.join(binary);
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            repo_root().join(relative).as_os_str(),
            "--std".as_ref(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    (dir, exe)
}

/// Run a command with no locale, and hand back the whole result.
///
/// `LC_ALL=C` for the reason `sort_agrees_with_gnu_sort` gives — there is
/// no locale anywhere in this language — and here it decides the
/// *wording*, not only the ordering: GNU quotes a bad argument `'zzz'`
/// under POSIX and `‘zzz’` under a UTF-8 locale, so an unpinned
/// comparison passes on one machine and fails on the next. It did:
/// this box defaults to POSIX and the CI runner does not
/// (`docs/standard-error.md` §7.1).
///
/// Ours is run the same way and ignores it, which is the point.
fn run_without_locale(command: &Path, args: &[&str], input: &str) -> std::process::Output {
    let mut child = Command::new(command)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the program runs");
    let text = input.to_owned();
    let mut stdin = child.stdin.take().expect("a piped stdin");
    std::thread::spawn(move || {
        let _ = stdin.write_all(text.as_bytes());
    });
    child.wait_with_output().expect("the program finishes")
}

/// A reference tool's diagnostic, with its own name taken off the front.
///
/// GNU takes that name from `argv[0]`, so a tool invoked by absolute path
/// calls itself `/usr/bin/cut` where this repository's programs carry a
/// literal. That difference is not wording, it is an open question
/// (`docs/standard-error.md` §8), so the comparison is of what follows.
///
/// Only the first line: GNU often adds a second pointing at `--help`.
fn complaint(stream: &[u8]) -> String {
    String::from_utf8_lossy(stream)
        .lines()
        .next()
        .unwrap_or_default()
        .split_once(": ")
        .map_or_else(String::new, |(_, rest)| rest.to_owned())
}

/// Whether the system's tool at `path` is GNU coreutils' rather than
/// BSD's, which is a different program with different wording.
fn is_gnu(path: &Path) -> bool {
    Command::new(path)
        .arg("--version")
        .output()
        .map(|v| v.status.success() && String::from_utf8_lossy(&v.stdout).contains("GNU coreutils"))
        .unwrap_or(false)
}

/// `docs/standard-error.md` §7, and `file-handles.md` §1.2's other half.
///
/// That section measured a `sort` which answered a file it could not read
/// and a file too large to hold with the same silence, distinguishable
/// only by an exit status: *"still not distinguishable to a person,
/// because neither prints anything"*. One of the two is now, and this is
/// it — the other needs a file past 1 GiB and 1,025 MB of resident
/// memory to reach, so it stays a hand check (§7).
#[test]
fn sort_names_the_file_it_cannot_read() {
    let (dir, exe) = build_example("example-sort-missing", "examples/sort/sort.ls", "sort");
    let missing = dir.join("no-such-file.txt");
    let path = missing.to_string_lossy().into_owned();

    let ours = run_without_locale(&exe, &[&path], "");
    assert_eq!(ours.status.code(), Some(2), "a file that is not there should exit 2");
    assert!(ours.stdout.is_empty(), "a failed read should print no data");
    assert_eq!(
        String::from_utf8_lossy(&ours.stderr),
        format!("sort: cannot read: {path}\n"),
        "a failed read should name the file on standard error"
    );

    let reference = Path::new("/usr/bin/sort");
    if is_gnu(reference) {
        let theirs = run_without_locale(reference, &[&path], "");
        assert_eq!(ours.status.code(), theirs.status.code(), "GNU sort exits differently");
        // GNU appends the errno string, which `fs_read` does not carry
        // (§6, and `file-handles.md` §3's `Failed(int)` is the shape
        // that would). Everything before it is the same claim.
        assert!(
            complaint(&theirs.stderr).starts_with(&complaint(&ours.stderr)),
            "GNU sort words it differently: {}",
            complaint(&theirs.stderr)
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/standard-error.md` §7: the third program, and the one where the
/// diagnostic is a single line with nothing in it that varies.
///
/// `base64 -d` exits 1 on a byte outside the alphabet — it always did —
/// and said nothing while doing it, which is the row §1 measured at 22
/// bytes for GNU and 0 for this.
#[test]
fn base64_reports_invalid_input_like_gnu_base64() {
    let (dir, exe) = build_example("example-base64-bad", "examples/base64/base64.ls", "base64");

    let ours = run_without_locale(&exe, &["-d"], "a@b\n");
    assert_eq!(ours.status.code(), Some(1), "a byte outside the alphabet should exit 1");
    assert_eq!(
        String::from_utf8_lossy(&ours.stderr),
        "base64: invalid input\n",
        "a refused decode should say so on standard error"
    );

    // And a valid decode stays silent, which is the half a new stream
    // makes it possible to get wrong.
    let good = run_without_locale(&exe, &["-d"], "aGVsbG8K\n");
    assert_eq!(good.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&good.stdout), "hello\n");
    assert!(good.stderr.is_empty(), "a successful decode should say nothing");

    let reference = Path::new("/usr/bin/base64");
    if is_gnu(reference) {
        let theirs = run_without_locale(reference, &["-d"], "a@b\n");
        assert_eq!(ours.status.code(), theirs.status.code(), "GNU base64 exits differently");
        assert_eq!(
            complaint(&ours.stderr),
            complaint(&theirs.stderr),
            "GNU base64 words it differently"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/agent-errors.md` §4: every independent refusal, not the first.
///
/// Three ill-typed functions in three independent bodies. Before this,
/// one was reported and the other two cost a compile, a read and a turn
/// each — which is almost free for a person with the file open and is
/// the whole cost for a program that has to ask again.
#[test]
fn every_independent_refusal_is_reported() {
    let dir = scratch("batched-refusals");
    let source = dir.join("three.ls");
    std::fs::write(
        &source,
        "fn a() -> [] int { return true; }\n\
         fn b() -> [] int { return oops; }\n\
         fn c() -> [] bool { return 1; }\n\
         fn main() -> [] int { return 0; }\n",
    )
    .expect("the fixture is written");

    let prose = Command::new(BIN).arg("check").arg(&source).output().expect("the compiler runs");
    let text = String::from_utf8_lossy(&prose.stderr);
    assert_eq!(
        text.matches("error:").count(),
        3,
        "three independent bodies should give three refusals:\n{text}"
    );
    // In source order, which is the order checking already runs in.
    let first = text.find("found `bool`").expect("a's refusal");
    let second = text.find("`oops` is not bound").expect("b's refusal");
    let third = text.find("found `int`").expect("c's refusal");
    assert!(first < second && second < third, "refusals are out of source order:\n{text}");

    // And the machine-readable form agrees, object for object.
    let json = Command::new(BIN)
        .arg("check")
        .arg(&source)
        .args(["--output", "json"])
        .output()
        .expect("the compiler runs");
    let body = String::from_utf8_lossy(&json.stdout);
    assert_eq!(body.matches("\"rule\":").count(), 3, "the JSON lost a refusal:\n{body}");
    assert_eq!(json.status.code(), Some(1), "a refused program still exits 1");

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/agent-errors.md` §6: the JSON adds fields beside the prose and
/// rewrites none of it.
///
/// The sentence a person reads is the sentence the object carries, so a
/// consumer of one is never reading something the other does not say —
/// and nobody has to keep two wordings in step.
#[test]
fn the_prose_is_unchanged_by_the_json() {
    for path in fixtures("reject") {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let prose = Command::new(BIN).arg("check").arg(&path).output().expect("the compiler runs");
        let prose = String::from_utf8_lossy(&prose.stderr).into_owned();
        let json = Command::new(BIN)
            .arg("check")
            .arg(&path)
            .args(["--output", "json"])
            .output()
            .expect("the compiler runs");
        let body = String::from_utf8_lossy(&json.stdout);
        let message = body
            .lines()
            .find_map(|line| line.trim().strip_prefix("\"message\": \""))
            .map(|rest| rest.trim_end_matches("\","))
            .unwrap_or_else(|| panic!("`{name}` reported no message:\n{body}"));
        // The JSON escapes what JSON must; compare what survives that.
        let message = message.replace("\\\"", "\"").replace("\\\\", "\\");
        assert!(
            prose.contains(&message),
            "`{name}` says one thing as prose and another as data:\n{prose}\n{message}"
        );
    }
}

/// A program with nothing wrong answers an empty list and exits 0.
///
/// The shape a consumer checks the length of, rather than an absence it
/// has to special-case.
#[test]
fn a_clean_program_answers_an_empty_list() {
    let out = Command::new(BIN)
        .arg("check")
        .arg(repo_root().join("examples/hello.ls"))
        .args(["--output", "json"])
        .output()
        .expect("the compiler runs");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "{\n  \"refused\": []\n}\n");
}

/// `AGENTS.md`'s checked code blocks are fixtures.
///
/// The document is a contract with whoever writes lex-sys next, and a
/// contract nothing enforces is a stale paragraph. So every block
/// marked `lex-sys` must compile, and every block marked
/// `lex-sys-refused` must be refused **with the rule it names** — the
/// same `//~ RULE` line the fixtures under `tests/reject/` carry
/// (`docs/agent-errors.md` §3).
///
/// Untagged blocks are shell, signatures or tables, and are not run.
/// A block that makes a claim about the language is tagged.
#[test]
fn the_agent_guidelines_compile_as_written() {
    let text =
        std::fs::read_to_string(repo_root().join("AGENTS.md")).expect("AGENTS.md is readable");
    let dir = scratch("agent-guidelines");

    let mut checked = 0;
    let mut rest = text.as_str();
    while let Some(start) = rest.find("```lex-sys") {
        let after = &rest[start + 3..];
        let (kind, body_start) = match after.strip_prefix("lex-sys-refused\n") {
            Some(body) => ("refused", body),
            None => match after.strip_prefix("lex-sys\n") {
                Some(body) => ("accepted", body),
                // `lex-sys` as the start of some other word: skip it.
                None => {
                    rest = &rest[start + 3..];
                    continue;
                }
            },
        };
        let end = body_start.find("```").expect("an unterminated code block in AGENTS.md");
        let body = &body_start[..end];
        rest = &body_start[end..];

        let path = dir.join(format!("block{checked}.ls"));
        std::fs::write(&path, body).expect("the block is written");
        let out = Command::new(BIN)
            .arg("check")
            .arg(&path)
            .args(["--std", "--output", "json"])
            .output()
            .expect("the compiler runs");
        let json = String::from_utf8_lossy(&out.stdout);
        let reported = json
            .lines()
            .find_map(|line| line.trim().strip_prefix("\"rule\": \""))
            .map(|r| r.trim_end_matches("\","));

        match kind {
            "accepted" => assert!(
                out.status.success(),
                "AGENTS.md block {checked} is shown as valid lex-sys and is not:\n{body}\n{json}"
            ),
            _ => {
                let declared = body
                    .lines()
                    .find_map(|line| line.trim().strip_prefix("//~ RULE "))
                    .map(str::trim)
                    .unwrap_or_else(|| {
                        panic!("AGENTS.md block {checked} is shown as refused and names no rule")
                    });
                assert_eq!(
                    reported,
                    Some(declared),
                    "AGENTS.md block {checked} names `{declared}`:\n{body}\n{json}"
                );
            }
        }
        checked += 1;
    }

    // A document with no checked blocks would pass this test while
    // saying nothing, which is the failure mode it exists to prevent.
    assert!(checked >= 5, "only {checked} checked blocks in AGENTS.md");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The compiler carries the guidelines, so a reader with the binary
/// needs no checkout.
#[test]
fn agent_guidelines_prints_the_file() {
    let out = Command::new(BIN).arg("agent-guidelines").output().expect("the compiler runs");
    assert!(out.status.success());
    let printed = String::from_utf8_lossy(&out.stdout);
    let onedisk = std::fs::read_to_string(repo_root().join("AGENTS.md")).expect("readable");
    assert_eq!(printed, onedisk, "the embedded guidelines have drifted from the file");
}

/// `docs/line-reading.md` §2: a line longer than the buffer.
///
/// The dimension the eight field specs never varied. Before the fix,
/// a 60,000-byte line lost its delimiters, so `cut` switched to the
/// no-delimiter rule and printed 60 KB of the wrong field with exit 0 —
/// a silently wrong answer in a program checked against GNU.
#[test]
fn cut_reports_a_long_line_like_gnu_cut() {
    let (dir, exe) = build_example("example-cut-long", "examples/cut/cut.ls", "cut");

    let reference = Path::new("/usr/bin/cut");
    let have_reference = is_gnu(reference);

    // Around the old 60,000-byte arena slice, and well past it.
    for first in [59_998usize, 59_999, 60_000, 70_000, 200_000] {
        let input = format!("{},second\n", "a".repeat(first));
        let ours = run_without_locale(&exe, &["-d,", "-f2"], &input);
        assert!(ours.status.success(), "a long line should not be an error");
        assert_eq!(
            String::from_utf8_lossy(&ours.stdout),
            "second\n",
            "a {first}-byte first field should not change which field comes back"
        );
        if have_reference {
            let theirs = run_without_locale(reference, &["-d,", "-f2"], &input);
            assert_eq!(
                String::from_utf8_lossy(&ours.stdout),
                String::from_utf8_lossy(&theirs.stdout),
                "GNU cut answers differently on a {first}-byte field"
            );
        }
    }

    // And the field itself, when it is the long one: the whole of it.
    let input = format!("first,{}\n", "b".repeat(100_000));
    let ours = run_without_locale(&exe, &["-d,", "-f2"], &input);
    assert_eq!(
        String::from_utf8_lossy(&ours.stdout).trim_end().len(),
        100_000,
        "a long field should come back whole"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/float-math.md` §2: `sqrt` is correctly rounded, and the two
/// hand-rolled roots it replaced were not.
///
/// Checked against Rust's own `f64::sqrt` — the same instruction, so
/// this is a test that the builtin reaches it rather than a test of the
/// hardware. The values include the four exponents where the twenty-step
/// Newton loop in `benches/game/spectral.ls` was wrong by 10^43 and
/// more, which is the failure this replaced.
#[test]
fn sqrt_agrees_with_the_hardware() {
    let dir = scratch("float-sqrt");

    // A deterministic spread: the specials, a decade sweep, and values
    // across the exponent range where the old loop fell short.
    let mut values: Vec<f64> = vec![0.0, 1.0, 2.0, 0.25, 1e-300, 1e-8, 1e8, 1e100, 1e200, 1e300];
    let mut seed = 0x5eed_u64;
    for _ in 0..2000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let mantissa = f64::from(((seed >> 11) & 0xff_ffff) as u32) / 16_777_216.0;
        let exponent = ((seed >> 40) % 600) as i32 - 300;
        values.push(libm_ldexp(mantissa + 0.5, exponent));
    }

    /// `mantissa * 2^exponent`, without pulling in a dependency.
    fn libm_ldexp(mantissa: f64, exponent: i32) -> f64 {
        let mut out = mantissa;
        let mut n = exponent;
        while n > 0 {
            out *= 2.0;
            n -= 1;
        }
        while n < 0 {
            out /= 2.0;
            n += 1;
        }
        out
    }

    // The program prints `sqrt` of each value, shortest-round-trip, one
    // per line — so a disagreement in the last bit is visible.
    let mut program = String::from(
        "import std.fmt;\nimport std.io;\n\n\
         fn show[&i](i: &!i Io, x: float) -> [io_write] int {\n\
         \x20   region a {\n\
         \x20       let out = alloc_slice[a](32, byte_of(0));\n\
         \x20       let n = fmt.float_into(out, x);\n\
         \x20       io.write_all(i, out[0..n]);\n\
         \x20   }\n\
         \x20   return io.newline(i);\n\
         }\n\n\
         fn main(world: World) -> [] int {\n\
         \x20   let Split { io, ffi, fs, heap, args } = split(world);\n\
         \x20   release(ffi); release(fs); release(heap); release(args);\n\
         \x20   borrow mut io as &!i in {\n",
    );
    for v in &values {
        program.push_str(&format!("        show(i, sqrt({v:e}));\n"));
    }
    program.push_str("    }\n    release(io);\n    return 0;\n}\n");

    let source = dir.join("sqrt.ls");
    std::fs::write(&source, &program).expect("the program is written");
    let exe = dir.join("sqrt");
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

    let out = Command::new(&exe).output().expect("it runs");
    let printed = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = printed.lines().collect();
    assert_eq!(lines.len(), values.len(), "one line per value");

    for (line, value) in lines.iter().zip(&values) {
        let got: f64 = line.parse().unwrap_or_else(|_| panic!("`{line}` is not a float"));
        let want = value.sqrt();
        assert_eq!(
            got.to_bits(),
            want.to_bits(),
            "sqrt({value:e}) came back {got:e}, and the hardware says {want:e}"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}
