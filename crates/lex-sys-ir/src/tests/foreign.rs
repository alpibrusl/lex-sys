use super::tests::lower_src;
use super::{Callee, Expr, Program, Stmt};

/// libc's `long labs(long)`, which is the smallest foreign function that
/// takes an argument, returns a result and cannot be mistaken for a
/// builtin.
const LABS: &str = "extern fn labs[&f](ffi: &f Ffi(\"libc\"), n: int) -> [ffi(\"libc\")] int; ";

fn refused(src: &str) -> String {
    lower_src(src).expect_err("this should be refused").message
}

fn accepted(src: &str) -> Program {
    lower_src(src).expect("this should be accepted")
}

/// A `main` that takes the authority it is given and gives it back, for
/// the cases whose subject is a declaration rather than a body.
const MAIN: &str = " fn main(world: World) -> [] int { \
        let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(fs); release(ffi); release(io); return 0; }";

/// A `main` that narrows to libc, runs `body` inside the borrow, and
/// gives everything back.
fn with_libc(body: &str) -> String {
    format!(
        "{LABS} fn main(world: World) -> [] int {{ \
             let Split {{ io, ffi, fs, heap, args }} = split(world); release(args); release(heap); release(fs); release(io); \
             let libc = narrow(ffi, \"libc\"); var n = 0; \
             borrow libc as &f in {{ {body} }} \
             release(libc); return n; }}"
    )
}

#[test]
fn a_foreign_call_needs_the_capability_that_names_its_library() {
    // §8.4, and the reason the declaration is where this is checked: it
    // is the only place a foreign signature is written.
    let message = refused(
        "extern fn labs(n: int) -> [ffi(\"libc\")] int; \
             fn main(world: World) -> [] int { let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(fs); \
             release(ffi); release(io); return labs(0); }",
    );
    assert!(message.contains("holds no capability that authorises it"), "{message}");
}

#[test]
fn a_foreign_row_is_exact_in_both_directions() {
    let quiet =
        refused(&format!("extern fn labs[&f](ffi: &f Ffi(\"libc\"), n: int) -> [] int;{MAIN}"));
    assert!(quiet.contains("does not declare"), "{quiet}");

    let wrong_library = refused(&format!(
        "extern fn labs[&f](ffi: &f Ffi(\"libc\"), n: int) -> [ffi(\"libm\")] int;{MAIN}"
    ));
    assert!(wrong_library.contains("holds no capability that authorises it"), "{wrong_library}");
}

#[test]
fn a_foreign_declaration_names_a_library() {
    // The unnarrowed root names none, so a declaration borrowing one
    // would be a foreign call with no library behind it.
    let message = refused(&format!(
        "extern fn labs[&f](ffi: &f Ffi(\"\"), n: int) -> [ffi(\"\")] int;{MAIN}"
    ));
    assert!(message.contains("names no library"), "{message}");
}

#[test]
fn only_what_c_can_name_crosses_the_boundary() {
    let aggregate = refused(&format!(
        "struct P {{ x: int }} \
             extern fn f[&c](ffi: &c Ffi(\"libc\"), p: P) -> [ffi(\"libc\")] int;{MAIN}"
    ));
    assert!(aggregate.contains("no agreed layout"), "{aggregate}");

    let reference = refused(&format!(
        "struct P {{ x: int }} \
             extern fn f[&c, &r](ffi: &c Ffi(\"libc\"), p: &r P) -> [ffi(\"libc\")] int;{MAIN}"
    ));
    assert!(reference.contains("borrowed capability"), "{reference}");
}

#[test]
fn a_foreign_name_is_not_also_a_written_function() {
    // Two answers to one call is one answer too many.
    let message = refused(&format!("{LABS} fn labs(n: int) -> [] int {{ return n; }}{MAIN}"));
    assert!(message.contains("already declared foreign"), "{message}");
}

#[test]
fn narrowing_goes_one_way() {
    // §7.4: prefix extension, and `libc` is a prefix of `libcrypto`, so
    // the capability over `libcrypto` is the narrower of the two.
    let message = refused(
        "fn main(world: World) -> [] int { let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(fs); \
             release(io); let crypto = narrow(ffi, \"libcrypto\"); \
             let wider = narrow(crypto, \"libc\"); release(wider); return 0; }",
    );
    assert!(message.contains("never widened"), "{message}");
}

#[test]
fn narrowing_to_the_same_thing_is_refused() {
    let message = refused(
        "fn main(world: World) -> [] int { let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(fs); \
             release(io); let a = narrow(ffi, \"libc\"); let b = narrow(a, \"libc\"); \
             release(b); return 0; }",
    );
    assert!(message.contains("grants nothing new"), "{message}");
}

#[test]
fn narrowing_consumes_the_wider_capability() {
    // The point of the whole section: after narrowing there is no way
    // back to what was narrowed, because it was spent.
    let message = refused(
        "fn main(world: World) -> [] int { let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(fs); \
             release(io); let libc = narrow(ffi, \"libc\"); release(libc); \
             let libm = narrow(ffi, \"libm\"); release(libm); return 0; }",
    );
    assert!(message.contains("has already been consumed"), "{message}");
}

#[test]
fn a_borrowed_capability_cannot_be_narrowed() {
    let message = refused(
        "fn main(world: World) -> [] int { let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(fs); \
             release(io); borrow ffi as &f in { let libc = narrow(f, \"libc\"); release(libc); } \
             release(ffi); return 0; }",
    );
    assert!(message.contains("narrowing consumes"), "{message}");
}

#[test]
fn owning_the_world_discharges_every_library() {
    // §8.2's discharge rule, with a label that carries a value. `main`
    // owns the `World`, which can be split and narrowed to anything, so
    // the authority for `ffi("libc")` is already in its parameter list.
    let program = accepted(&with_libc("n = labs(f, 0 - 7);"));
    let main = program.func(program.find("main").expect("main"));
    assert!(main.effects.is_pure(), "{}", main.effects);
}

#[test]
fn a_borrowed_narrowed_capability_declares_the_label_it_names() {
    let program = accepted(&format!(
        "{LABS} \
             fn size[&f](ffi: &f Ffi(\"libc\"), n: int) -> [ffi(\"libc\")] int \
             {{ return labs(ffi, n); }} \
             fn main(world: World) -> [] int {{ let Split {{ io, ffi, fs, heap, args }} = split(world); release(args); release(heap); release(fs); \
             release(io); let libc = narrow(ffi, \"libc\"); var n = 0; \
             borrow libc as &f in {{ n = size(f, 0 - 7); }} release(libc); return n - 7; }}"
    ));
    let size = program.func(program.find("size").expect("size"));
    assert_eq!(size.effects.to_string(), "[ffi(\"libc\")]");

    // And it is not interchangeable with the label for another library.
    let message = refused(&format!(
        "{LABS} \
             fn size[&f](ffi: &f Ffi(\"libc\"), n: int) -> [ffi(\"libm\")] int \
             {{ return labs(ffi, n); }}{MAIN}"
    ));
    assert!(message.contains("does not declare"), "{message}");
}

#[test]
fn the_capability_travels_as_far_as_the_check_and_no_further() {
    // The IR keeps the capability as an argument, because a capability's
    // *journey* is what was checked and the argument still has to be
    // evaluated. Dropping it is the backend's job (§8.1), and
    // `tests/accept/narrowed_capability.ls` is what proves it happens.
    let program = accepted(&with_libc("n = labs(f, 0 - 7);"));
    let main = program.func(program.find("main").expect("main"));
    let mut foreign_calls = 0;
    for stmt in main.body.iter() {
        walk(stmt, &mut foreign_calls);
    }
    assert_eq!(foreign_calls, 1, "the foreign call should survive lowering");
}

/// Count `Callee::Extern` calls, checking each one's shape as it goes.
fn walk(stmt: &Stmt, found: &mut u32) {
    let mut visit = |expr: &Expr| {
        if let Expr::Call { callee: Callee::Extern(_), args } = expr {
            assert_eq!(args.len(), 2, "the capability is still an argument here");
            *found += 1;
        }
    };
    match stmt {
        Stmt::Store { value, .. } | Stmt::Eval(value) | Stmt::Return(value) => visit(value),
        Stmt::Borrow { body, .. } => {
            for inner in body {
                walk(inner, found);
            }
        }
        _ => {}
    }
}
