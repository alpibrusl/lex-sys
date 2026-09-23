use super::tests::lower_src;
use super::{Expr, Program, Stmt};

fn refused(src: &str) -> String {
    lower_src(src).expect_err("this should be refused").message
}

fn accepted(src: &str) -> Program {
    lower_src(src).expect("this should be accepted")
}

fn in_main(body: &str) -> String {
    format!("fn main() -> [] int {{ {body} return 0; }}")
}

#[test]
fn a_literal_is_a_shared_static_byte_slice() {
    // §1 and §4: a string is `&static [byte]`, so every slice rule
    // applies to it and none of them had to be written twice.
    accepted(&in_main("let s = \"hi\"; let n = len(s); let first = s[0]; let c = int_of(first);"));

    // Shared: two occurrences may be the same bytes, so nothing writes
    // through one.
    let message = refused(&in_main("let s = \"hi\"; s[0] = byte_of(65);"));
    assert!(message.contains("shared slice"), "{message}");
}

#[test]
fn static_outlives_every_region_and_is_not_declarable() {
    // A literal may be handed back to any caller, because its bytes are
    // in the object file rather than in a frame.
    accepted(
        "fn greeting() -> [] &static [byte] { return \"hi\"; } \
                  fn main() -> [] int { return len(greeting()) - 2; }",
    );

    // A buffer in an arena may not: same occurs-check, same code.
    let escaped = refused(
        "fn build() -> [] &static [byte] \
             { region a { return alloc_slice[a](2, byte_of(65)); } } \
             fn main() -> [] int { return 0; }",
    );
    assert!(escaped.contains("may not outlive its region"), "{escaped}");

    // And `static` is the one region with a name rather than a binder,
    // so it cannot be declared as a parameter.
    let declared = refused(
        "fn f[&static](x: &static [byte]) -> [] int { return 0; } \
             fn main() -> [] int { return 0; }",
    );
    assert!(declared.contains("cannot be declared"), "{declared}");

    // Nor written unique: there is no unique reference into it.
    let unique = refused(
        "fn f(x: &!static [byte]) -> [] int { return 0; } \
             fn main() -> [] int { return 0; }",
    );
    assert!(unique.contains("shared"), "{unique}");
}

#[test]
fn a_byte_is_storage_and_not_arithmetic() {
    // §2, and the decision that keeps `defined-behaviour.md` §8's
    // deferral of unsigned widths intact.
    for expression in ["b + b", "b - b", "b * b", "b / b", "0 - b"] {
        let message = refused(&format!(
            "fn f(b: byte) -> [] byte {{ return {expression}; }} \
                 fn main() -> [] int {{ return 0; }}"
        ));
        assert!(message.contains("byte"), "{expression}: {message}");
    }

    // Comparison is allowed: comparing storage is not arithmetic.
    accepted(
        "fn f(b: byte) -> [] bool { return b == byte_of(44); } \
             fn main() -> [] int { return 0; }",
    );

    // And a byte is not an int, in either direction, without saying so.
    let widened =
        refused("fn f(b: byte) -> [] int { return b; } fn main() -> [] int { return 0; }");
    assert!(widened.contains("expected `int`, found `byte`"), "{widened}");
    accepted("fn f(b: byte) -> [] int { return int_of(b); } fn main() -> [] int { return 0; }");
}

#[test]
fn a_byte_slice_crosses_to_c_and_other_references_do_not() {
    // §6: a pointer and a separate length, because C has no notion of
    // the pair.
    accepted(
        "extern fn write[&f, &s](ffi: &f Ffi(\"libc\"), fd: int, buf: &s [byte], n: int) \
             -> [ffi(\"libc\")] int; \
             fn main() -> [] int { return 0; }",
    );

    // A reference to anything else still stops at the checker.
    let other = refused(
        "struct P { x: int } \
             extern fn f[&c, &r](ffi: &c Ffi(\"libc\"), p: &r P) -> [ffi(\"libc\")] int; \
             fn main() -> [] int { return 0; }",
    );
    assert!(other.contains("borrowed capability"), "{other}");
}

#[test]
fn a_literal_carries_its_bytes_and_not_its_spelling() {
    // `canonical-ast.md` §3: the AST keeps values, not spellings, so an
    // escape is resolved by the parser and never reaches the tree.
    let program = accepted(&in_main("let s = \"a\\nb\"; let n = len(s);"));
    let main = program.func(program.find("main").expect("main"));
    // Read the node rather than its `Debug` rendering, which would
    // escape the newline straight back again.
    let Some(Stmt::Store { value: Expr::Bytes(text), .. }) = main.body.first() else {
        panic!("a literal, got {:?}", main.body.first())
    };
    assert_eq!(text, "a\nb", "the escape should already be resolved");
    assert_eq!(text.len(), 3, "three bytes, not four");
}
