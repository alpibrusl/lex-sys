//! The M0 conformance harness.
//!
//! Every fixture under `tests/accept/` must compile, run, and produce the
//! output its header declares. Every fixture under `tests/reject/` must be
//! refused, with the message its header declares.
//!
//! The header syntax is a comment the compiler ignores:
//!
//! ```text
//! //~ STDOUT <a line the program must print>
//! //~ EXIT <the status it must exit with>      (default 0)
//! //~ ERROR <a substring the refusal must contain>
//! ```
//!
//! Adding a rule to the language means adding a fixture here. M2 says every
//! rule needs a must-reject fixture (#1, #2); the discipline starts at M0, when
//! it is cheap.

use std::path::{Path, PathBuf};
use std::process::Command;

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

#[test]
fn accepted_programs_build_and_run() {
    for path in fixtures("accept") {
        let source = std::fs::read_to_string(&path).expect("a readable fixture");
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let dir = scratch(&name);
        let exe = dir.join(&name);

        let build = Command::new(BIN)
            .args(["build".as_ref(), path.as_os_str(), "-o".as_ref(), exe.as_os_str()])
            .output()
            .expect("the compiler runs");
        assert!(
            build.status.success(),
            "`{name}` should compile, but the compiler said:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );

        let run = Command::new(&exe).output().expect("the compiled program runs");

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
        let build = Command::new(BIN)
            .args(["build".as_ref(), path.as_os_str(), "-o".as_ref(), exe.as_os_str()])
            .output()
            .expect("the compiler runs");
        assert!(
            build.status.success(),
            "`{name}` should compile, but the compiler said:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );

        let run = Command::new(&exe).output().expect("the compiled example runs");
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
    for dir in ["tests/accept", "tests/reject", "examples"] {
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
    "fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io] int {\n\
         var n = 0;\n\
         while n < len(s) { putchar(io, int_of(s[n])); n = n + 1; }\n\
         return len(s);\n\
     }\n\
     fn print_nat[&i](io: &!i Io, n: int) -> [io] int {\n\
         if n >= 10 { print_nat(io, n / 10); }\n\
         return putchar(io, 48 + n % 10);\n\
     }\n\
     fn run[&a, &i](args: &a Args, io: &!i Io) -> [args, io] int {\n\
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
