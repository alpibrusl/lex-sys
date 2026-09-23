use super::tests::lower_src;
use super::{Effects, Label, Program};

fn refused(src: &str) -> String {
    lower_src(src).expect_err("this should be refused").message
}

fn accepted(src: &str) -> Program {
    lower_src(src).expect("this should be accepted")
}

#[test]
fn a_row_is_a_canonically_ordered_set() {
    // §7.1: no duplicates, and an order that does not depend on which
    // label a file happened to mention first.
    let a = Effects::plain(["io", "fs", "io"]);
    let b = Effects::plain(["fs", "io"]);
    assert_eq!(a, b);
    assert_eq!(a.to_string(), "[fs, io]");
    // §7.4: a label's argument is part of its identity, and the order is
    // still the text's rather than the order of mention.
    let narrowed = Effects::new([
        Label { name: "ffi".to_owned(), argument: Some("libm".to_owned()) },
        Label { name: "ffi".to_owned(), argument: Some("libc".to_owned()) },
        Label { name: "ffi".to_owned(), argument: Some("libc".to_owned()) },
    ]);
    assert_eq!(narrowed.to_string(), "[ffi(\"libc\"), ffi(\"libm\")]");
}

#[test]
fn union_and_subset_are_the_only_operations_needed() {
    let mut row = Effects::pure();
    assert!(row.is_pure());
    row.union(&Effects::plain(["io"]));
    row.union(&Effects::plain(["io", "fs"]));
    assert_eq!(row.to_string(), "[fs, io]");
    assert_eq!(row.missing_from(&Effects::plain(["fs", "io"])), None);
    assert_eq!(
        row.missing_from(&Effects::plain(["io"])).map(Label::to_string),
        Some("fs".to_owned())
    );
}

#[test]
fn a_call_widens_the_callers_row() {
    let message = refused(
        "fn quiet[&i](io: &!i Io) -> [] int { putchar(io, 65); return 0; } \
             fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("performs `io_write`"), "{message}");
}

#[test]
fn an_over_wide_row_is_an_error_not_a_warning() {
    let message =
        refused("fn f() -> [io_write] int { return 1; } fn main() -> [] int { return 0; }");
    assert!(message.contains("never performs it"), "{message}");
}

#[test]
fn a_row_is_transitive() {
    let message = refused(
        "fn shout[&i](io: &!i Io) -> [io_write] int { return putchar(io, 33); } \
             fn caller[&i](io: &!i Io) -> [] int { return shout(io); }",
    );
    assert!(message.contains("`caller` performs `io_write`"), "{message}");
    accepted(
        "fn shout[&i](io: &!i Io) -> [io_write] int { return putchar(io, 33); } \
             fn caller[&i](io: &!i Io) -> [io_write] int { return shout(io); }",
    );
}

#[test]
fn an_ungrounded_label_can_never_be_exact() {
    // No registry of legal labels, and none needed: nothing performs
    // `telepathy`, so no exact row can contain it.
    let message =
        refused("fn f() -> [telepathy] int { return 1; } fn main() -> [] int { return 0; }");
    assert!(message.contains("declares `telepathy`"), "{message}");
}

#[test]
fn a_duplicate_label_is_the_same_row() {
    accepted(
        "fn f[&i](io: &!i Io) -> [io_write, io_write] int { return putchar(io, 33); } \
             fn caller[&i](io: &!i Io) -> [io_write] int { return f(io); }",
    );
}

#[test]
fn a_pure_helper_inside_an_effectful_body_adds_nothing() {
    accepted(
        "fn double(n: int) -> [] int { return n * 2; } \
             fn main[&i](io: &!i Io) -> [io_write] int { return putchar(io, double(20)); }",
    );
}

#[test]
fn an_effect_in_a_branch_still_counts() {
    // The row is a union over the calls the body contains, not over the
    // ones a particular run reaches. Anything else would need to know
    // which branch is taken.
    let message = refused(
        "fn f[&i](io: &!i Io, c: bool) -> [] int { if c { putchar(io, 65); } return 0; } \
             fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("performs `io_write`"), "{message}");
}

#[test]
fn a_generic_functions_row_is_not_per_instantiation() {
    // Rows live on signatures, so monomorphisation does not touch them:
    // one row however many copies the backend emits.
    let program = accepted(
        "fn id[T](x: T) -> [] T { return x; } \
             fn main[&i](io: &!i Io) -> [io_write] int { return putchar(io, id(65)) - 65; }",
    );
    let copies: Vec<&str> =
        program.funcs.iter().map(|f| f.name.as_str()).filter(|n| n.starts_with("id")).collect();
    assert_eq!(copies.len(), 1, "{copies:?}");
    let id = program.func(program.find(copies[0]).expect("id"));
    assert!(id.effects.is_pure(), "{}", id.effects);
}

#[test]
fn the_row_reaches_the_lowered_function() {
    let program =
        accepted("fn main[&i](io: &!i Io) -> [io_write] int { return putchar(io, 65) - 65; }");
    let main = program.func(program.find("main").expect("main"));
    assert_eq!(main.effects.to_string(), "[io_write]");
}
