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
/// should have refused this": a program outside what this backend lowers
/// is refused with a located `CodegenError`, never a panic. `region`/
/// `alloc_slice` moved out of this list once §7.5 landed; bare
/// `alloc[a]`/`box`/`unbox` moved out once §7.15 landed. `Type::Float`
/// is still such a gap -- `tests/accept/floating_point.ls` needed it
/// and nothing else to stay outside this backend.
#[test]
fn a_program_outside_this_backend_is_refused_not_panicked() {
    let source = "\
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(fs);
    release(ffi);
    release(io);
    release(heap);
    let x = 1.0 / 2.0;
    return 0;
}
";
    let ast = parse(source).expect("parses");
    let program = lex_sys_ir::lower(&ast).expect("type-checks");
    let error = compile_object(&program, "main")
        .expect_err("`Type::Float` is not part of this backend yet");
    assert!(error.message.contains("Float"), "{}", error.message);
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

/// `docs/llvm-backend.md` §5's third slice: `tests/accept/llvm_control.ls`
/// exercises `if`/`else`, `while`, all six comparisons and both
/// short-circuit operators. The exact bytes prove the loop ran the right
/// number of times and that `&&`/`||` skipped `shout` exactly when they
/// should have, not only that the module compiled.
#[test]
fn control_flow_builds_and_runs_the_control_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("accept")
        .join("llvm_control.ls");
    let source = std::fs::read_to_string(&path).expect("the fixture exists");
    let object = compiled(&source, "main");
    let output = run(&object, "control");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "01X34Z\n+-42\nF\n!T\nT\n!T\n",
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

/// `docs/llvm-backend.md` §5's fourth slice: `examples/hello.ls` -- the
/// program §5 originally (and wrongly) named as the first slice's own
/// target -- builds and runs, closing the loop `ci.yml`'s smoke test
/// opened.
#[test]
fn hello_ls_builds_and_runs_end_to_end() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("hello.ls");
    let source = std::fs::read_to_string(&path).expect("the example exists");
    let object = compiled(&source, "main");
    let output = run(&object, "hello");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Hello, world!\n",
        "the LLVM backend printed the wrong thing"
    );
    assert_eq!(output.status.code(), Some(0), "the LLVM backend exited wrongly");
}

/// `docs/llvm-backend.md` §5's fifth slice: `tests/accept/enums.ls`
/// exercises a struct literal, an enum with payloads (including a
/// struct-typed payload), and `match` -- a chain of tag tests, a
/// wildcard arm, and a matched binding read back through plain
/// `Expr::Field`, since a by-value match binds an owned struct, not a
/// reference to one.
#[test]
fn structs_and_enums_build_and_run_the_enums_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("accept")
        .join("enums.ls");
    let source = std::fs::read_to_string(&path).expect("the fixture exists");
    let object = compiled(&source, "main");
    let output = run(&object, "enums");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "001220069901\n",
        "the LLVM backend computed the wrong values"
    );
    assert_eq!(output.status.code(), Some(0), "the LLVM backend exited wrongly");
}

/// `docs/llvm-backend.md` §7.11: `Place::Field`/`Place::Deref`, plus the
/// read-side siblings (`Expr::FieldRef`/`Expr::FieldAddr`/`Expr::Deref`)
/// -- `tests/accept/deref_roundtrip.ls` exercises a shared and a unique
/// reference to a bare `int` (`*n`, `*n = e`), a field read *through* a
/// reference (`scale`'s own `p.x`/`p.y`), and the whole referent
/// replaced (`*p = Point { .. }`), all in one fixture.
#[test]
fn field_and_deref_writes_build_and_run_the_deref_roundtrip_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("accept")
        .join("deref_roundtrip.ls");
    let source = std::fs::read_to_string(&path).expect("the fixture exists");
    let object = compiled(&source, "main");
    let output = run(&object, "deref-roundtrip");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "41 42\n3 4 -> 30 40\n",
        "the LLVM backend computed the wrong values"
    );
    assert_eq!(output.status.code(), Some(0), "the LLVM backend exited wrongly");
}

/// `tests/accept/arguments.ls`, run with no arguments (`run` passes none):
/// `argc` is still `1`, its own name, exactly as C hands it over
/// (`docs/arguments.md` §3). The conformance suite's differential test
/// passes real arguments; this fixture's own contract only covers the
/// no-argument case.
#[test]
fn arg_count_and_arg_build_and_run_the_arguments_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("accept")
        .join("arguments.ls");
    let source = std::fs::read_to_string(&path).expect("the fixture exists");
    let object = compiled(&source, "main");
    let output = run(&object, "arguments");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "1\nnamed: 1\n",
        "the LLVM backend computed the wrong argc/argv"
    );
    assert_eq!(output.status.code(), Some(0), "the LLVM backend exited wrongly");
}

/// `tests/accept/arena_roundtrip.ls`: `alloc[a](value)`, single-value
/// bump allocation -- the same `bump` helper `alloc_slice` already
/// opened (§7.5), minus its fill loop.
#[test]
fn alloc_builds_and_runs_the_arena_roundtrip_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("accept")
        .join("arena_roundtrip.ls");
    let source = std::fs::read_to_string(&path).expect("the fixture exists");
    let object = compiled(&source, "main");
    let output = run(&object, "arena-roundtrip");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "0 1 4 9 16 25 36 49 = 140\nnested: 7\n",
        "the LLVM backend computed the wrong values"
    );
    assert_eq!(output.status.code(), Some(0), "the LLVM backend exited wrongly");
}

/// `tests/accept/box_roundtrip.ls`: `box(h, value)`/`unbox(h, b)`, the
/// heap-shaped twin of `alloc` above -- one `malloc`, trapping on
/// exhaustion exactly as `boxed_slice` already does, and one `free` on
/// the way out, the load happening first.
#[test]
fn box_and_unbox_build_and_run_the_box_roundtrip_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("accept")
        .join("box_roundtrip.ls");
    let source = std::fs::read_to_string(&path).expect("the fixture exists");
    let object = compiled(&source, "main");
    let output = run(&object, "box-roundtrip");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "7\n10 4\n",
        "the LLVM backend computed the wrong values"
    );
    assert_eq!(output.status.code(), Some(0), "the LLVM backend exited wrongly");
}

/// `s[i]` is bounds-checked (`docs/defined-behaviour.md` §1); `uge`
/// catches both ends with one comparison, so a negative index and one
/// past the end are the same check on real hardware, not only on paper.
fn assert_indexing_traps(index: &str, tag: &str) {
    let source = format!(
        "fn main(world: World) -> [] int {{\n\
             let Split {{ io, ffi, fs, heap, args }} = split(world);\n\
             release(args); release(heap); release(fs); release(ffi); release(io);\n\
             let s = \"abc\";\n\
             return int_of(s[{index}]);\n\
         }}\n"
    );
    let object = compiled(&source, "main");
    let output = run(&object, tag);
    assert_eq!(output.status.code(), None, "`s[{index}]` should be killed by a signal, not exit");
    assert_eq!(
        output.status.signal(),
        Some(4),
        "`s[{index}]` should trap with SIGILL, matching Cranelift's own signal for a bounds \
         check (docs/llvm-backend.md §3.2)"
    );
}

#[test]
fn indexing_past_a_slice_traps_with_sigill() {
    assert_indexing_traps("5", "index-past");
}

#[test]
fn indexing_before_a_slice_traps_with_sigill() {
    assert_indexing_traps("0 - 1", "index-before");
}

/// `docs/llvm-backend.md` §7.5: `region`/`alloc_slice` -- exhausting the
/// arena's chunk traps rather than handing back a slice past the end,
/// matching `lex-sys-codegen`'s own `bump`. 9000 `int`s is 72000 bytes,
/// past `ARENA_CHUNK`'s 65536.
#[test]
fn allocating_past_an_arenas_chunk_traps_with_sigill() {
    let source = "\
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(fs); release(ffi); release(io); release(heap);
    region a {
        let s = alloc_slice[a](9000, 0);
        return s[0];
    }
}
";
    let object = compiled(source, "main");
    let output = run(&object, "arena-exhausted");
    assert_eq!(output.status.code(), None, "an exhausted arena should be killed by a signal");
    assert_eq!(
        output.status.signal(),
        Some(4),
        "an exhausted arena should trap with SIGILL, matching Cranelift's own signal \
         (docs/llvm-backend.md §3.2)"
    );
}

/// `docs/slicing.md`, `docs/llvm-backend.md` §7.9: `s[a..b]` traps
/// rather than yielding a silently wrong answer, on either of its two
/// bad shapes -- past the slice's own length, or inverted (`start >
/// end`) -- matching `lex-sys-codegen`'s own `subslice`.
fn assert_subslicing_traps(range: &str, tag: &str) {
    let source = format!(
        "fn main(world: World) -> [] int {{\n\
             let Split {{ io, ffi, fs, heap, args }} = split(world);\n\
             release(args); release(heap); release(fs); release(ffi); release(io);\n\
             let s = \"abc\";\n\
             let t = s[{range}];\n\
             return len(t);\n\
         }}\n"
    );
    let object = compiled(&source, "main");
    let output = run(&object, tag);
    assert_eq!(output.status.code(), None, "`s[{range}]` should be killed by a signal, not exit");
    assert_eq!(
        output.status.signal(),
        Some(4),
        "`s[{range}]` should trap with SIGILL, matching Cranelift's own signal for a bad \
         subslice (docs/llvm-backend.md §3.2)"
    );
}

#[test]
fn subslicing_past_the_end_traps_with_sigill() {
    assert_subslicing_traps("0..5", "subslice-past");
}

#[test]
fn an_inverted_subslice_traps_with_sigill() {
    assert_subslicing_traps("2..0", "subslice-inverted");
}
