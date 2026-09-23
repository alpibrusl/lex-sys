use super::tests::lower_src;
use super::{Builtin, Callee, Expr, Program, Stmt};

fn refused(src: &str) -> String {
    lower_src(src).expect_err("this should be refused").message
}

fn accepted(src: &str) -> Program {
    lower_src(src).expect("this should be accepted")
}

#[test]
fn wrapping_arithmetic_is_spelled_out() {
    // §2.2: `+` means arithmetic, `wrapping_add` means the bits. The
    // asymmetry is what stops the second happening by accident.
    let program = accepted(
        "fn f(a: int, b: int) -> [] int { return wrapping_add(a, wrapping_mul(b, 2)); } \
             fn main() -> [] int { return f(1, 2) - 5; }",
    );
    let f = program.func(program.find("f").expect("f"));
    let Some(Stmt::Return(Expr::Call { callee, .. })) = f.body.first() else {
        panic!("a call, got {:?}", f.body.first())
    };
    assert_eq!(*callee, Callee::Builtin(Builtin::WrappingAdd));
}

#[test]
fn wrapping_arithmetic_is_pure_and_needs_no_capability() {
    // It observes nothing outside the program, so it is not an effect
    // and an empty row still means pure.
    let program = accepted(
        "fn f(a: int) -> [] int { return wrapping_mul(a, 3); } \
             fn main() -> [] int { return f(0); }",
    );
    let f = program.func(program.find("f").expect("f"));
    assert!(f.effects.is_pure(), "{}", f.effects);
}

#[test]
fn a_wrapping_builtin_cannot_be_redefined() {
    let message = refused(
        "fn wrapping_add(a: int, b: int) -> [] int { return a; } \
             fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("is a builtin"), "{message}");
}

#[test]
fn a_struct_literal_runs_in_the_order_it_is_written() {
    // §3: the order you read is the order it runs, kept true by
    // refusing the literal that would break it rather than by
    // reordering underneath the text.
    let message = refused(
        "struct P { x: int, y: int } \
             fn main() -> [] int { let p = P { y: 2, x: 1 }; return p.x; }",
    );
    assert!(message.contains("declaration order"), "{message}");

    // Written in declaration order, the same literal is fine.
    accepted(
        "struct P { x: int, y: int } \
             fn main() -> [] int { let p = P { x: 1, y: 2 }; return p.x - 1; }",
    );

    // A single field, or fields that skip none, are unaffected: the rule
    // is about relative order, not about naming every field in a row.
    accepted(
        "struct Q { a: int, b: int, c: int } \
             fn main() -> [] int { let q = Q { a: 1, b: 2, c: 3 }; return q.a - 1; }",
    );
}
