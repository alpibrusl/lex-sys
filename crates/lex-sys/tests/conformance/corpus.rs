//! The corpus: every accept and reject fixture, every example, and the command line around them.

use super::*;

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
    //
    // And one no source file can reach on purpose: `internal` names the
    // compiler's failures, and a fixture that produced one would be a bug
    // report. `docs/internal-errors.md` §5: it is covered by the unit
    // tests in `lex-sys` and `lex-sys-codegen`, which break the IR by hand.
    const COVERED_ELSEWHERE: [&str; 2] = ["not-public", "internal"];

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
fn a_wrong_command_line_is_a_usage_error_not_a_refusal() {
    let output = Command::new(BIN).arg("frobnicate").output().expect("the compiler runs");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
}
