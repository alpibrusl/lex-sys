use super::tests::lower_src;
use super::{Program, Stmt};

const NODE: &str = "struct Node { value: int } ";

fn refused(src: &str) -> String {
    lower_src(src).expect_err("this should be refused").message
}

fn accepted(src: &str) -> Program {
    lower_src(src).expect("this should be accepted")
}

/// `main` with `body` inside, for the cases about a region rather than
/// about a signature.
fn in_main(body: &str) -> String {
    format!("{NODE} fn main() -> [] int {{ {body} return 0; }}")
}

#[test]
fn an_arena_is_a_region_and_alloc_hands_back_a_unique_reference() {
    let program =
        accepted(&in_main("region a { let n = alloc[a](Node { value: 1 }); let v = n.value; }"));
    let main = program.func(program.find("main").expect("main"));
    assert!(
        matches!(main.body.first(), Some(Stmt::Region { arena: 0, .. })),
        "a `region` lowers to an arena, got {:?}",
        main.body.first()
    );
}

#[test]
fn nothing_mentioning_the_arena_escapes_it() {
    // §6, and the same occurs-check §5 runs -- which is the claim: an
    // arena's lifetime and a borrow's lifetime are one mechanism.
    let message = refused(&format!(
        "{NODE} fn escape[&q](fallback: &q Node) -> [] &q Node \
             {{ region a {{ return alloc[a](Node {{ value: 1 }}); }} }} \
             fn main() -> [] int {{ return 0; }}"
    ));
    assert!(message.contains("may not outlive its region"), "{message}");
}

#[test]
fn an_inner_arenas_reference_may_not_be_stored_in_an_outer_one() {
    // Nesting is §5.2's stack. The outer arena outlives the inner, so
    // references go inwards and never back out.
    let message = refused(&in_main(
        "region o { var held = alloc[o](Node { value: 1 }); \
             region i { held = alloc[i](Node { value: 2 }); } }",
    ));
    assert!(message.contains("does not outlive"), "{message}");

    // And the permitted direction is genuinely permitted.
    accepted(&in_main(
        "region o { let base = alloc[o](Node { value: 1 }); \
             region i { let n = base.value + alloc[i](Node { value: 2 }).value; } }",
    ));
}

#[test]
fn an_arena_holds_val_data_only() {
    // §6.1: releasing an arena reclaims memory and runs nothing, so a
    // linear obligation put inside would be dropped rather than
    // discharged -- a leak with a static blessing.
    let message = refused(
        "res struct Ticket { fd: int } \
             fn open(n: int) -> [] Ticket { return Ticket { fd: n }; } \
             fn main() -> [] int { let f = open(3); region a { let p = alloc[a](f); } return 0; }",
    );
    assert!(message.contains("arena holds `val` data only"), "{message}");
}

#[test]
fn alloc_names_an_arena_that_is_open() {
    let unopened = refused(&in_main("let n = alloc[a](Node { value: 1 });"));
    assert!(unopened.contains("is not an arena open here"), "{unopened}");

    // A `borrow` block's region is a region, but it is not an arena:
    // there is no chunk behind it to allocate in.
    let borrowed = refused(&format!(
        "{NODE} fn main() -> [] int {{ let b = Node {{ value: 1 }}; \
             borrow b as &r in {{ let n = alloc[r](Node {{ value: 2 }}); }} return 0; }}"
    ));
    assert!(borrowed.contains("is not an arena open here"), "{borrowed}");

    // Nor is a region *parameter*: a caller's region is not this
    // function's to allocate in.
    let parameter = refused(&format!(
        "{NODE} fn f[&q](n: &q Node) -> [] int {{ let p = alloc[q](Node {{ value: 1 }}); return 0; }} \
             fn main() -> [] int {{ return 0; }}"
    ));
    assert!(parameter.contains("is not an arena open here"), "{parameter}");
}

#[test]
fn an_inner_arena_shadows_an_outer_one_of_the_same_name() {
    // Two arenas, two regions: `Region::Block` carries identity rather
    // than depth, so the inner `a` is not the outer `a`.
    let program =
        accepted(&in_main("region a { region a { let n = alloc[a](Node { value: 1 }); } }"));
    let main = program.func(program.find("main").expect("main"));
    let Some(Stmt::Region { arena: 0, body }) = main.body.first() else {
        panic!("the outer arena, got {:?}", main.body.first());
    };
    assert!(
        matches!(body.first(), Some(Stmt::Region { arena: 1, .. })),
        "the inner arena is a second one, got {:?}",
        body.first()
    );
}

#[test]
fn a_unique_reference_is_accepted_where_a_shared_one_is_wanted() {
    // §6's one new coercion, and what makes arena data reachable from a
    // helper written against `&r`: `&!r T` is `&r T` plus permission to
    // write, so handing one over read-only gives away nothing.
    accepted(&format!(
        "{NODE} fn value_of[&r](n: &r Node) -> [] int {{ return n.value; }} \
             fn main() -> [] int {{ region a {{ let v = value_of(alloc[a](Node {{ value: 1 }})); }} return 0; }}"
    ));

    // The other direction stays refused: a shared reference promises the
    // referent will not change, and nothing may write through it.
    let message = refused(&format!(
        "{NODE} fn bump[&r](n: &!r Node) -> [] int {{ n.value = 1; return 0; }} \
             fn main() -> [] int {{ let b = Node {{ value: 1 }}; \
             borrow b as &r in {{ let x = bump(r); }} return 0; }}"
    ));
    assert!(message.contains("unique reference"), "{message}");
}

#[test]
fn an_arena_reference_is_val_and_costs_one_pointer() {
    // A reference is `val` whatever it points at (§5 rule 3), so a
    // binding holding one owes nothing at scope end. Two of them from
    // the same arena is not a double anything.
    accepted(&in_main(
        "region a { let x = alloc[a](Node { value: 1 }); let y = x; let z = x.value; }",
    ));
}
