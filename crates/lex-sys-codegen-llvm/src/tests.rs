//! `docs/llvm-backend.md` §5: the doorway, checked end to end against a
//! real `clang` on the host running these tests.

use std::path::Path;
use std::process::Command;

use lex_sys_syntax::parse;

use super::*;

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
