//! `docs/llvm-backend.md` §5: the doorway, checked end to end against a
//! real `clang` on the host running these tests.

use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::Command;

use lex_sys_syntax::parse;

use super::*;

/// A `main` that returns `expr`, where `expr` may use `x` -- a value the
/// checker cannot fold, because it comes back from `probe`, an impure call
/// (`putchar` performs `io_write`).
///
/// A trap-signal test needs this: `docs/compile-time.md` §3 folds an
/// operator whose *both* operands are literals at compile time, and turns
/// one that would overflow into a refused program (`Rule::ConstantTraps`)
/// rather than a running one -- correct for a `static`, wrong for what
/// this backend's own checked-arithmetic codegen is supposed to be tested
/// against. `x` is always `0` at run time (`putchar` echoes back the byte
/// it wrote), so every expression below reads as the constant it would be
/// if it were foldable -- it is only kept unfoldable on purpose.
fn program_returning(expr: &str) -> String {
    format!(
        "fn probe[&i](io: &!i Io) -> [io_write] int {{\n\
             return putchar(io, 0);\n\
         }}\n\
         fn main(world: World) -> [] int {{\n\
             let Split {{ io, ffi, fs, heap, args }} = split(world);\n\
             release(args); release(heap); release(fs); release(ffi);\n\
             var x = 0;\n\
             borrow mut io as &!i in {{\n\
                 x = probe(i);\n\
             }}\n\
             release(io);\n\
             return {expr};\n\
         }}\n"
    )
}

/// `docs/llvm-backend.md` §3.2's own finding, applied to this backend's
/// own suite rather than left as a gap in someone else's: a runtime trap
/// is checked by its **signal**, `SIGILL` (4), not only by
/// `status.code() == None` -- which is true of every signal alike and
/// would not have caught §3.2's `SIGTRAP` bug either.
fn assert_traps_with_sigill(expr: &str, tag: &str) {
    let source = program_returning(expr);
    let object = compiled(&source, "main");
    let output = run(&object, tag);
    assert_eq!(output.status.code(), None, "`{expr}` should be killed by a signal, not exit");
    assert_eq!(
        output.status.signal(),
        Some(4),
        "`{expr}` should trap with SIGILL, matching Cranelift's own signal for a checked-\
         arithmetic trap (docs/llvm-backend.md §3.2)"
    );
}

fn compiled(source: &str, entry: &str) -> Vec<u8> {
    let ast = parse(source).expect("the fixture parses");
    let program = lex_sys_ir::lower(&ast).expect("the fixture type-checks");
    compile_object(&program, entry).expect("the LLVM backend should lower this fixture")
}

fn run(object: &[u8], tag: &str) -> std::process::Output {
    let dir =
        std::env::temp_dir().join(format!("lex-sys-codegen-llvm-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a writable temporary directory");
    let obj = dir.join("out.o");
    let exe = dir.join("out");
    std::fs::write(&obj, object).expect("a writable object file");

    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_owned());
    let link = Command::new(&cc).arg(&obj).arg("-o").arg(&exe).status().expect("the linker runs");
    assert!(link.success(), "linking the LLVM-emitted object failed");

    let output = Command::new(&exe).output().expect("the linked program runs");
    let _ = std::fs::remove_dir_all(&dir);
    output
}

/// The fixture `tests/accept/llvm_smoke.ls` builds and runs identically
/// through `lex-sys build --backend llvm`, checked here directly against
/// the backend's own doorway rather than through the CLI.
#[test]
fn the_first_slice_builds_and_runs_the_smoke_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("accept")
        .join("llvm_smoke.ls");
    let source = std::fs::read_to_string(&path).expect("the fixture exists");
    let object = compiled(&source, "main");
    let output = run(&object, "smoke");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Hi!\n",
        "the LLVM backend printed the wrong thing"
    );
    assert_eq!(output.status.code(), Some(0), "the LLVM backend exited wrongly");
}

/// `docs/internal-errors.md`'s promise applies to this backend too, even
/// though its gaps are "not implemented yet" rather than "the checker
/// should have refused this": a program outside the first slice is
/// refused with a located `CodegenError`, never a panic.
#[test]
fn a_program_outside_the_first_slice_is_refused_not_panicked() {
    let source = "\
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    var n = 0;
    while n < 3 {
        n = n + 1;
    }
    return n;
}
";
    let ast = parse(source).expect("parses");
    let program = lex_sys_ir::lower(&ast).expect("type-checks");
    let error = compile_object(&program, "main")
        .expect_err("a `while` loop is not part of the first slice");
    assert!(error.message.contains("first slice"), "{}", error.message);
}

/// `docs/llvm-backend.md` §5's second slice: `tests/accept/llvm_arith.ls`
/// exercises every trapping `BinOp` plus the three bitwise operators, each
/// once, and the exact bytes prove the values are right, not only that
/// `clang` accepted the module.
#[test]
fn checked_arithmetic_builds_and_runs_the_arith_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("accept")
        .join("llvm_arith.ls");
    let source = std::fs::read_to_string(&path).expect("the fixture exists");
    let object = compiled(&source, "main");
    let output = run(&object, "arith");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Hi! OK$iK\n",
        "the LLVM backend computed the wrong values"
    );
    assert_eq!(output.status.code(), Some(0), "the LLVM backend exited wrongly");
}

/// `Add`/`Sub`/`Mul` overflow: LLVM's own `with.overflow` intrinsics,
/// checked against the exact boundary Cranelift's `sadd_overflow` traps
/// on.
#[test]
fn checked_add_traps_on_overflow() {
    assert_traps_with_sigill("x + 9223372036854775807 + 1", "add-overflow");
}

#[test]
fn checked_sub_traps_on_overflow() {
    assert_traps_with_sigill("(-9223372036854775808 + x) - 1", "sub-overflow");
}

#[test]
fn checked_mul_traps_on_overflow() {
    assert_traps_with_sigill("(4611686018427387904 + x) * 2", "mul-overflow");
}

/// `Div`/`Rem`: LLVM's `sdiv`/`srem` are undefined, not trapping, on
/// these two inputs (`docs/llvm-backend.md` §5's own finding) -- these
/// four tests are what proves the manual checks ahead of the instruction
/// actually run, on a real `clang`, rather than merely compile.
#[test]
fn checked_div_traps_on_division_by_zero() {
    assert_traps_with_sigill("10 / x", "div-zero");
}

#[test]
fn checked_div_traps_on_int_min_over_negative_one() {
    assert_traps_with_sigill("(-9223372036854775808 + x) / (0 - 1)", "div-intmin");
}

#[test]
fn checked_rem_traps_on_division_by_zero() {
    assert_traps_with_sigill("10 % x", "rem-zero");
}

/// `Shl`/`Shr`: an amount outside `0..64` traps rather than being masked.
#[test]
fn checked_shl_traps_on_an_out_of_range_amount() {
    assert_traps_with_sigill("1 << (64 + x)", "shl-range");
}

#[test]
fn checked_shr_traps_on_a_negative_amount() {
    assert_traps_with_sigill("1 >> (x - 1)", "shr-range");
}
