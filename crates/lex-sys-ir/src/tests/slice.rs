use super::Program;
use super::tests::lower_src;

fn refused(src: &str) -> String {
    lower_src(src).expect_err("this should be refused").message
}

fn accepted(src: &str) -> Program {
    lower_src(src).expect("this should be accepted")
}

/// `main` with `body` inside, for the cases about a region rather than
/// about a signature.
fn in_main(body: &str) -> String {
    format!("fn main() -> [] int {{ {body} return 0; }}")
}

#[test]
fn a_slice_is_a_reference_and_carries_its_region() {
    accepted(&in_main("region a { let xs = alloc_slice[a](3, 0); xs[0] = 1; let n = xs[0]; }"));

    // And it cannot outlive the arena it points into -- the same
    // occurs-check that stops a `borrow`'s reference escaping, run by
    // the same code. A slice adds no escape rule of its own.
    let message = refused(
        "fn escape[&q](fallback: &q [int]) -> [] &q [int] \
             { region a { return alloc_slice[a](1, 0); } } \
             fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("may not outlive its region"), "{message}");
}

#[test]
fn an_unsized_referent_is_not_a_value() {
    // `[T]`'s length is a runtime value rather than part of its type,
    // so there is nothing to lay out. Only a reference may point at one.
    for written in [
        "fn f(xs: [int]) -> [] int { return 0; }",
        "struct S { xs: [int] }",
        "fn f[&r](xs: &r [[int]]) -> [] int { return 0; }",
    ] {
        let message = refused(&format!("{written} fn main() -> [] int {{ return 0; }}"));
        assert!(message.contains("no size of its own"), "{written}: {message}");
    }
}

#[test]
fn a_slice_holds_val_data_only() {
    // §6.1, sharpened: the fill is copied into every element, and a
    // linear value cannot be copied at all.
    let message = refused(
        "res struct Ticket { fd: int } \
             fn open(n: int) -> [] Ticket { return Ticket { fd: n }; } \
             fn main() -> [] int { region a { let s = alloc_slice[a](2, open(1)); } return 0; }",
    );
    assert!(message.contains("arena holds `val` data only"), "{message}");
}

/// `docs/slicing.md` §1 and §4: `s[a..b]` is a slice of the same
/// element type, region and **mode**.
#[test]
fn a_subslice_keeps_the_bases_mode() {
    // A shared base gives a shared slice, usable where `&r [T]` is.
    accepted(&in_main(
        "region a { let xs = alloc_slice[a](4, 0); let part = xs[1..3]; \
             let n = part[0]; }",
    ));

    // And a unique base keeps uniqueness, which §4 justifies from
    // `linearity-and-effects.md` §5: every reference derived from one
    // borrow points into the one buffer the referent was spilled to,
    // so overlapping subslices alias correctly rather than racing.
    // That is why there is no `split_at` here for *safety*.
    accepted(&in_main(
        "region a { let xs = alloc_slice[a](4, 0); let part = xs[0..3]; \
             part[0] = 1; let other = xs[1..4]; other[0] = 2; }",
    ));
}

/// §1: `..` takes a range of a **run**, so there has to be one.
#[test]
fn a_range_needs_a_slice_to_range_over() {
    let message = refused(&in_main("let n = 7; let part = n[0..2];"));
    assert!(message.contains("is not a slice"), "{message}");
}

#[test]
fn a_shared_slice_may_not_be_written_through() {
    let message = refused(
        "fn clobber[&r](xs: &r [int]) -> [] int { xs[0] = 1; return 0; } \
             fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("shared slice"), "{message}");

    // The unique one may, and the coercion the other way still holds:
    // a function wanting `&r [int]` accepts the `&!a [int]` `alloc_slice`
    // hands back.
    accepted(
        "fn total[&r](xs: &r [int]) -> [] int { return len(xs); } \
             fn main() -> [] int { region a { let xs = alloc_slice[a](2, 0); \
             xs[0] = 1; let n = total(xs); } return 0; }",
    );
}

#[test]
fn only_a_slice_can_be_indexed_or_measured() {
    let indexed = refused(&in_main("let x = 3; let y = x[0];"));
    assert!(indexed.contains("is not a slice"), "{indexed}");

    let measured = refused(&in_main("let x = 3; let n = len(x);"));
    assert!(measured.contains("is not a slice"), "{measured}");
}

#[test]
fn an_index_is_an_int_and_so_is_a_length() {
    let bad_index =
        refused(&in_main("region a { let xs = alloc_slice[a](2, 0); let n = xs[true]; }"));
    assert!(bad_index.contains("expected `int`"), "{bad_index}");

    let bad_count = refused(&in_main("region a { let xs = alloc_slice[a](true, 0); }"));
    assert!(bad_count.contains("expected `int`"), "{bad_count}");
}

#[test]
fn a_slices_element_type_comes_from_its_fill() {
    // No annotation anywhere: `alloc_slice[a](3, true)` is a slice of
    // `bool` because the fill is one, and indexing it yields `bool`.
    accepted(&in_main(
        "region a { let flags = alloc_slice[a](3, true); \
             if flags[0] { let n = 1; } }",
    ));

    // And the element type is invariant, like every other referent.
    let message = refused(
        "fn ints[&r](xs: &r [int]) -> [] int { return len(xs); } \
             fn main() -> [] int { region a { let flags = alloc_slice[a](2, true); \
             let n = ints(flags); } return 0; }",
    );
    assert!(message.contains("expected"), "{message}");
}
