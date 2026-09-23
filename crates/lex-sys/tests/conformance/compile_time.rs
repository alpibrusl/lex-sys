//! Compile-time evaluation: what folds, what stays a call, and what still traps.

use super::*;

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
