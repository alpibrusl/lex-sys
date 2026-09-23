//! The command line: `Args`, `arg`, and `std.flags`.

use super::*;

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
