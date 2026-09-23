use super::tests::lower_src;
use super::{Diagnostic, Program};

/// A `res` type, a way to make one, and a way to spend one -- the three
/// things every case below needs.
const PRELUDE: &str = "\
        res struct Ticket { fd: int } \
        fn open(n: int) -> [] Ticket { return Ticket { fd: n }; } \
        fn close(f: Ticket) -> [] int { let Ticket { fd } = f; return fd; } ";

fn check(body: &str) -> Result<Program, Diagnostic> {
    lower_src(&format!("{PRELUDE}{body}"))
}

fn refused(body: &str) -> String {
    check(body).expect_err("this should be refused").message
}

fn accepted(body: &str) {
    check(body).expect("this should be accepted");
}

#[test]
fn a_res_value_consumed_once_is_accepted() {
    accepted("fn main() -> [] int { return close(open(1)); }");
}

#[test]
fn a_res_value_used_twice_is_refused() {
    let message = refused(
        "struct Pair { a: Ticket, b: Ticket } \
             fn main() -> [] int { let f = open(1); let p = Pair { a: f, b: f }; return 0; }",
    );
    assert!(message.contains("already been consumed"), "{message}");
}

#[test]
fn a_res_value_used_after_a_move_is_refused() {
    let message =
        refused("fn main() -> [] int { let f = open(1); let a = close(f); return close(f); }");
    assert!(message.contains("already been consumed"), "{message}");
}

#[test]
fn a_res_value_live_at_a_return_is_refused() {
    let message = refused("fn main() -> [] int { let f = open(1); return 0; }");
    assert!(message.contains("consumed on every path"), "{message}");
}

#[test]
fn a_res_value_live_at_the_end_of_a_block_is_refused() {
    let message = refused("fn main() -> [] int { if true { let f = open(1); } return 0; }");
    assert!(message.contains("still live at the end of this block"), "{message}");
}

#[test]
fn branches_must_agree_about_what_is_live() {
    let message =
        refused("fn main() -> [] int { let f = open(1); if true { let a = close(f); } return 0; }");
    assert!(message.contains("branches disagree about `f`"), "{message}");
}

#[test]
fn branches_that_agree_are_accepted() {
    accepted(
        "fn main() -> [] int { let f = open(1); \
             if true { let a = close(f); } else { let b = close(f); } return 0; }",
    );
}

#[test]
fn an_arm_that_returns_does_not_have_to_agree() {
    // A `return` is not at the merge point, so it takes no part in the
    // join. Without that, §4.1's own accepting example would be refused.
    accepted(
        "fn main() -> [] int { let f = open(1); \
             if true { return close(f); } return close(f); }",
    );
}

#[test]
fn an_arm_may_create_and_spend_a_value_of_its_own() {
    // The `then` arm declares and consumes `f`; the empty `else` never
    // sees it. That is not a disagreement -- a binding declared inside an
    // arm dies with the arm, and its own block already checked it.
    accepted("fn main() -> [] int { if true { let f = open(1); let a = close(f); } return 0; }");
}

#[test]
fn a_loop_may_not_consume_an_outer_binding() {
    let message = refused(
        "fn main() -> [] int { let f = open(1); var i = 0; \
             while i < 2 { let a = close(f); i = i + 1; } return 0; }",
    );
    assert!(message.contains("consumed inside this loop"), "{message}");
}

#[test]
fn a_loop_that_consumes_what_it_creates_is_accepted() {
    accepted(
        "fn main() -> [] int { var i = 0; \
             while i < 2 { let f = open(i); let a = close(f); i = i + 1; } return 0; }",
    );
}

#[test]
fn a_conditionally_evaluated_operand_is_a_branch() {
    // `&&` does not evaluate its right operand when the left decides, so
    // a consumption there happens on one path only.
    let message = refused(
        "fn spend(f: Ticket) -> [] bool { let a = close(f); return true; } \
             fn main() -> [] int { let f = open(1); \
             let b = false && spend(f); return 0; }",
    );
    assert!(message.contains("branches disagree"), "{message}");
}

#[test]
fn a_res_value_cannot_be_discarded() {
    let message = refused("fn main() -> [] int { open(1); return 0; }");
    assert!(message.contains("cannot be discarded"), "{message}");
}

#[test]
fn a_val_value_may_be_discarded() {
    accepted("fn main() -> [] int { close(open(1)); return 0; }");
}

#[test]
fn a_field_cannot_be_read_out_of_a_res_value() {
    let message =
        refused("fn main() -> [] int { let f = open(1); let n = f.fd; return close(f); }");
    assert!(message.contains("a field cannot be read out of it"), "{message}");
}

#[test]
fn assigning_over_a_live_res_binding_is_refused() {
    let message = refused("fn main() -> [] int { var f = open(1); f = open(2); return close(f); }");
    assert!(message.contains("would discard the `res` value"), "{message}");
}

#[test]
fn assigning_over_a_spent_res_binding_is_accepted() {
    accepted(
        "fn main() -> [] int { var f = open(1); let a = close(f); f = open(2); \
             return close(f); }",
    );
}

#[test]
fn a_wildcard_arm_may_not_swallow_a_res_scrutinee() {
    let message = refused(
        "enum Slot { Empty, Full(Ticket) } \
             fn size(s: Slot) -> [] int { match s { Slot::Empty => { return 0; } \
             _ => { return 1; } } } fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("the value matched here is `res`"), "{message}");
}

#[test]
fn an_ignored_res_payload_is_refused() {
    let message = refused(
        "enum Slot { Empty, Full(Ticket) } \
             fn size(s: Slot) -> [] int { match s { Slot::Empty => { return 0; } \
             Slot::Full(_) => { return 1; } } } fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("this payload is `res`"), "{message}");
}

#[test]
fn an_ignored_val_payload_is_accepted() {
    accepted(
        "enum Slot { Empty, Full(int) } \
             fn size(s: Slot) -> [] int { match s { Slot::Empty => { return 0; } \
             Slot::Full(_) => { return 1; } } } fn main() -> [] int { return 0; }",
    );
}

#[test]
fn mode_is_inferred_from_members() {
    let message = refused(
        "struct Holder { f: Ticket } \
             fn main() -> [] int { let h = Holder { f: open(1) }; return 0; }",
    );
    assert!(message.contains("consumed on every path"), "{message}");
}

#[test]
fn a_val_declaration_may_not_hold_a_res_member() {
    let message = refused("val struct Wrapper { f: Ticket } fn main() -> [] int { return 0; }");
    assert!(message.contains("declared `val`, but it holds"), "{message}");
}

#[test]
fn a_val_declaration_of_val_members_is_accepted() {
    accepted("val struct Point { x: int, y: int } fn main() -> [] int { return 0; }");
}

#[test]
fn a_generic_type_takes_its_mode_from_its_arguments() {
    // `Held[int]` is `val` and may be dropped; `Held[Ticket]` is `res`.
    accepted(
        "struct Held[T] { value: T } fn main() -> [] int { let h = Held { value: 1 }; return 0; }",
    );
    let message = refused(
        "struct Held[T] { value: T } \
             fn main() -> [] int { let h = Held { value: open(1) }; return 0; }",
    );
    assert!(message.contains("consumed on every path"), "{message}");
}

/// `docs/mode-polymorphism.md` §3.1 and §4: where the error goes.
///
/// An unbounded parameter is checked as `res` — the stronger
/// obligation — so a body that drops it is refused **at the
/// definition**, which is where it is wrong. This used to be accepted
/// where it was written and refused at the copy, as
/// "(instantiated at `Ticket`)".
///
/// A function that meant only copyable types says `[T: val]`, and
/// then the refusal moves to the call site, where the choice of type
/// was actually made.
#[test]
fn where_a_generic_that_drops_its_parameter_is_refused() {
    let definition =
        refused("fn sink[T](x: T) -> [] int { return 0; } fn main() -> [] int { return sink(1); }");
    assert!(definition.contains("is still live"), "{definition}");
    assert!(
        !definition.contains("instantiated at"),
        "the rigid check is the definition, not an instantiation: {definition}"
    );

    // With the bound, the definition is fine and the *caller* is not.
    accepted(
        "fn sink[T: val](x: T) -> [] int { return 0; } \
             fn main() -> [] int { return sink(1); }",
    );
    let call_site = refused(
        "fn sink[T: val](x: T) -> [] int { return 0; } \
             fn main() -> [] int { return sink(open(1)); }",
    );
    assert!(call_site.contains("needs `T` to be `val`"), "{call_site}");
    assert!(call_site.contains("`Ticket` is `res`"), "{call_site}");
}

/// `docs/collections.md` §3: a `res` aggregate may bound its own
/// parameters, and the bound is kept where the argument is supplied.
///
/// This is the declaration `std.vec` needed and a `val`/`res` keyword
/// cannot express: the vector *owns* an allocation, so it is `res`,
/// while its elements have to be copyable, because a boxed slice
/// holds `val` data only. The keyword speaks about the aggregate;
/// this is about a parameter.
#[test]
fn a_res_aggregate_may_bound_its_parameters() {
    accepted(
        "res struct Vec[T: val] { held: Box[[T]], used: int } \
             fn size[T: val, &v](v: &v Vec[T]) -> [] int { return v.used; } \
             fn main() -> [] int { return 0; }",
    );

    // And the refusal lands where the caller wrote the type argument,
    // not inside the library at some `box_slice` it cannot change.
    let message = refused(
        "res struct Vec[T: val] { held: Box[[T]], used: int } \
             fn hold(v: Vec[Ticket]) -> [] int { return 0; } \
             fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("`Vec` bounds `T` by `val`"), "{message}");
    assert!(message.contains("`Ticket` is `res`"), "{message}");
}

/// The bug the bound was unusable for (`docs/collections.md` §4).
///
/// Keeping a declaration's bound reads the argument's mode, and it
/// read it against *nothing* -- so a rigid `T` came out `res` however
/// the enclosing function had bounded it, and a `[T: val]` function
/// could not name a `val` aggregate at `T` at all. The bound was
/// refused in exactly the position it exists for.
#[test]
fn a_bounded_parameter_may_stand_where_a_val_aggregate_wants_one() {
    accepted(
        "val struct Wrap[T] { held: T } \
             fn rewrap[T: val](w: Wrap[T]) -> [] T { let Wrap { held } = w; return held; } \
             fn main() -> [] int { return rewrap(Wrap { held: 7 }) - 7; }",
    );

    // Unbounded is still refused, and that is the point of the pair:
    // an unbounded `T` is checked as `res`, so `Wrap[T]` would be a
    // `val` aggregate holding a resource.
    let message = refused(
        "val struct Wrap[T] { held: T } \
             fn rewrap[T](w: Wrap[T]) -> [] T { let Wrap { held } = w; return held; } \
             fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("its type arguments are `val` too"), "{message}");
}

/// `docs/defer.md` §2: the expansion is real, so the exactly-once
/// rule is enforced on every exit path.
#[test]
fn a_defer_consumes_on_every_path() {
    // The shape §4.2 of `linearity-and-effects.md` calls verbose:
    // one resource, several exits, and no repetition now.
    accepted(
        "fn take(flag: bool) -> [] int { \
             let f = open(7); defer close(f); \
             if flag { return 2; } return 4; } \
             fn main() -> [] int { return 0; }",
    );

    // And it is the *same* rule underneath: consuming by hand as well
    // is the ordinary double consumption.
    let message = refused(
        "fn twice() -> [] int { let f = open(7); defer close(f); return close(f); } \
             fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("has already been consumed"), "{message}");

    // A `defer` that produces a resource leaks it, exactly as the
    // expression statement it becomes would.
    let leaked = refused(
        "fn leak() -> [] int { defer open(1); return 0; } \
             fn main() -> [] int { return 0; }",
    );
    assert!(leaked.contains("cannot be discarded"), "{leaked}");
}

/// §2.1: block scope, so a `defer` inside a `borrow` runs while the
/// referent is still frozen.
#[test]
fn a_defer_runs_inside_the_block_it_was_written_in() {
    let message = refused(
        "fn frozen() -> [] int { var f = open(7); \
             borrow f as &r in { defer close(f); } return close(f); } \
             fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("frozen by an enclosing `borrow`"), "{message}");
}

#[test]
fn destructuring_consumes_the_whole_and_produces_the_parts() {
    accepted(
        "struct Pair { a: Ticket, b: Ticket } \
             fn main() -> [] int { let p = Pair { a: open(1), b: open(2) }; \
             let Pair { a, b } = p; return close(a) + close(b); }",
    );
}

#[test]
fn a_destructured_part_carries_its_own_obligation() {
    let message = refused(
        "struct Pair { a: Ticket, b: Ticket } \
             fn main() -> [] int { let p = Pair { a: open(1), b: open(2) }; \
             let Pair { a, b } = p; return close(a); }",
    );
    assert!(message.contains("consumed on every path"), "{message}");
}

#[test]
fn a_partial_destructuring_is_refused() {
    let message = refused(
        "struct Pair { a: Ticket, b: Ticket } \
             fn main() -> [] int { let p = Pair { a: open(1), b: open(2) }; \
             let Pair { a } = p; return close(a); }",
    );
    assert!(message.contains("takes the whole value apart"), "{message}");
}

#[test]
fn destructuring_names_the_declared_fields() {
    let message =
        refused("fn main() -> [] int { let Ticket { handle } = open(1); return handle; }");
    assert!(message.contains("has no field `handle`"), "{message}");
    let message = refused("fn main() -> [] int { let Missing { x } = open(1); return x; }");
    assert!(message.contains("is not a struct"), "{message}");
}

#[test]
fn destructuring_evaluates_its_value_once() {
    // Two fields, one call: the value goes into an unnamed slot and the
    // parts come out of it.
    let program = check(
        "struct Pair { a: int, b: int } \
             fn make() -> [] Pair { return Pair { a: 1, b: 2 }; } \
             fn main() -> [] int { let Pair { a, b } = make(); return a + b; }",
    )
    .expect("accepted");
    let main = program.func(program.find("main").expect("main"));
    let calls = format!("{:?}", main.body).matches("Call").count();
    assert_eq!(calls, 1, "{:?}", main.body);
}

#[test]
fn an_enum_may_be_declared_res() {
    let message = refused(
        "res enum Handle { Closed, Open(int) } \
             fn main() -> [] int { let h = Handle::Closed; return 0; }",
    );
    assert!(message.contains("consumed on every path"), "{message}");
}

// ---- borrowing (`docs/linearity-and-effects.md` §5) -----------------

#[test]
fn a_borrow_block_that_returns_is_a_terminator() {
    // It runs once and unconditionally, so a function whose only `return`
    // is inside one has still returned. The backend agrees by asking the
    // same `terminates`.
    // A `val` referent, so nothing is owed when the block returns; a
    // `res` one would still have to be consumed on the way out, which is
    // a different rule doing its job.
    accepted(
        "struct C { n: int } \
             fn main() -> [] int { let c = C { n: 1 }; borrow c as &r in { return r.n - 1; } }",
    );
}

#[test]
fn a_shared_borrow_reads_without_consuming() {
    accepted(
        "fn size[&p](h: &p Ticket) -> [] int { return h.fd; } \
             fn main() -> [] int { let f = open(1); \
             borrow f as &r in { let n = size(r); } return close(f); }",
    );
}

#[test]
fn a_reference_reads_a_field_its_referent_could_not() {
    // The owned value refuses `f.fd` (a part read without taking the
    // whole apart); the reference is exactly how that read is spelled.
    accepted(
        "fn main() -> [] int { let f = open(1); \
             borrow f as &r in { let n = r.fd; } return close(f); }",
    );
    let message =
        refused("fn main() -> [] int { let f = open(1); let n = f.fd; return close(f); }");
    assert!(message.contains("a field cannot be read out of it"), "{message}");
}

#[test]
fn a_frozen_binding_cannot_be_moved() {
    let message = refused(
        "fn main() -> [] int { let f = open(1); \
             borrow f as &r in { let a = close(f); } return 0; }",
    );
    assert!(message.contains("frozen by an enclosing `borrow`"), "{message}");
}

#[test]
fn a_frozen_binding_cannot_be_assigned_to() {
    let message = refused(
        "struct C { n: int } fn main() -> [] int { var c = C { n: 1 }; \
             borrow c as &r in { c = C { n: 2 }; } return 0; }",
    );
    assert!(message.contains("cannot be assigned to"), "{message}");
}

#[test]
fn a_consumed_value_has_nothing_left_to_borrow() {
    let message = refused(
        "fn main() -> [] int { let f = open(1); let a = close(f); \
             borrow f as &r in { return a; } }",
    );
    assert!(message.contains("nothing left to borrow"), "{message}");
}

#[test]
fn the_freeze_lifts_when_the_block_closes() {
    accepted(
        "fn main() -> [] int { let f = open(1); \
             borrow f as &r in { let n = r.fd; } return close(f); }",
    );
}

#[test]
fn shared_borrows_nest() {
    // Freezing is not exclusive, and the inner block closing must not
    // thaw the outer one -- which is why the checker counts rather than
    // flags.
    accepted(
        "fn size[&p](h: &p Ticket) -> [] int { return h.fd; } \
             fn main() -> [] int { let f = open(1); \
             borrow f as &a in { borrow f as &b in { let n = size(a) + size(b); } \
             let m = size(a); } return close(f); }",
    );
}

#[test]
fn a_reference_may_not_outlive_its_region() {
    let message = refused(
        "fn escape[&q](f: Ticket, fallback: &q Ticket) -> [] &q Ticket { \
             borrow f as &r in { return r; } return fallback; } \
             fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("may not outlive its region"), "{message}");
}

#[test]
fn a_reference_may_not_escape_through_inference_either() {
    // A binding declared outside the block whose type was still a hole
    // when the block opened. No `return` is involved, which is why rule 4
    // is checked over every binding and not only over what leaves.
    let message = refused(
        "enum Holder[T] { Empty, Full(T) } \
             fn main() -> [] int { let f = open(1); let hole = Holder::Empty; \
             borrow f as &r in { let used: Holder[&r Ticket] = hole; } return close(f); }",
    );
    assert!(message.contains("would hold a reference into `r`"), "{message}");
}

#[test]
fn a_region_must_be_in_scope_where_it_is_written() {
    let message = refused(
        "fn escape(f: Ticket) -> [] &r Ticket { return f; } fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("is not a region in scope"), "{message}");
}

#[test]
fn sibling_regions_do_not_outlive_each_other() {
    let message = refused(
        "fn same[&p](a: &p Ticket, b: &p Ticket) -> [] int { return 0; } \
             fn u(x: Ticket, y: Ticket) -> [] int { \
             borrow x as &a in { borrow y as &b in { let n = same(a, b); } } \
             return close(x) + close(y); } fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("does not outlive"), "{message}");
}

#[test]
fn an_outer_reference_is_usable_in_an_inner_block() {
    accepted(
        "fn size[&p](h: &p Ticket) -> [] int { return h.fd; } \
             fn u(x: Ticket, y: Ticket) -> [] int { \
             borrow x as &a in { borrow y as &b in { let n = size(a) + size(b); } } \
             return close(x) + close(y); } fn main() -> [] int { return 0; }",
    );
}

#[test]
fn a_declared_outlives_is_checked_at_the_call_site() {
    const OUTER_FIRST: &str = "fn copy_into[&dst, &src where src <= dst](d: &dst Ticket, s: &src Ticket) -> [] int { return 0; } \
             fn u(x: Ticket, y: Ticket) -> [] int { \
             borrow x as &outer in { borrow y as &inner in { let n = copy_into(PAIR); } } \
             return close(x) + close(y); } fn main() -> [] int { return 0; }";
    // `dst` is the outer block, which does outlive the inner `src`.
    accepted(&OUTER_FIRST.replace("PAIR", "outer, inner"));
    // And the other way round, which does not.
    let message = refused(&OUTER_FIRST.replace("PAIR", "inner, outer"));
    assert!(message.contains("requires `src <= dst`"), "{message}");
}

#[test]
fn a_where_clause_names_the_declarations_own_regions() {
    let message = refused(
        "fn f[&a where b <= a](x: &a Ticket) -> [] int { return 0; } fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("is not a region parameter"), "{message}");
}

#[test]
fn region_and_type_parameters_do_not_collide() {
    let message =
        refused("fn f[T, &T](x: T) -> [] int { return 0; } fn main() -> [] int { return 0; }");
    assert!(message.contains("both a type parameter and a region parameter"), "{message}");
    let message = refused(
        "fn f[&r, &r](x: &r Ticket) -> [] int { return 0; } fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("region parameter `r` is declared twice"), "{message}");
}

#[test]
fn a_reference_is_val_and_may_be_copied_and_dropped() {
    // §5 rule 3. A reference to a `res` value is still `val`, which is
    // sound because the referent is frozen for the whole region.
    accepted(
        "fn main() -> [] int { let f = open(1); \
             borrow f as &r in { let a = r; let b = r; let n = a.fd + b.fd; } \
             return close(f); }",
    );
}

#[test]
fn a_unique_borrow_locks_its_referent() {
    // §5 rule 2: nothing else may touch it at all, not even a read.
    let message = refused(
        "struct C { n: int } fn main() -> [] int { var c = C { n: 1 }; \
             borrow mut c as &!r in { let peek = c; } return 0; }",
    );
    assert!(message.contains("nothing else may read it"), "{message}");
}

#[test]
fn there_is_at_most_one_unique_borrow() {
    let message = refused(
        "struct C { n: int } fn main() -> [] int { var c = C { n: 1 }; \
             borrow mut c as &!a in { borrow mut c as &!b in { return 0; } } }",
    );
    assert!(message.contains("already uniquely borrowed"), "{message}");
}

#[test]
fn a_frozen_value_cannot_be_borrowed_uniquely() {
    let message = refused(
        "struct C { n: int } fn main() -> [] int { var c = C { n: 1 }; \
             borrow c as &s in { borrow mut c as &!u in { return 0; } } }",
    );
    assert!(message.contains("cannot be borrowed uniquely"), "{message}");
}

#[test]
fn a_unique_borrow_releases_its_lock_at_the_end_of_the_block() {
    accepted(
        "struct C { n: int } fn main() -> [] int { var c = C { n: 1 }; \
             borrow mut c as &!a in { a.n = 2; } \
             borrow mut c as &!b in { b.n = 3; } return c.n - 3; }",
    );
}

#[test]
fn copies_of_one_unique_reference_alias() {
    // `&!r` is `val`, so it copies. Both copies are copies of one
    // pointer, so the second write sees the first. Spilling the value in
    // and reading it back per copy would lose one of them, which is why
    // there is a single buffer per block rather than one per reference.
    accepted(
        "struct C { n: int } fn main() -> [] int { var c = C { n: 0 }; \
             borrow mut c as &!r in { let a = r; let b = r; a.n = 3; b.n = b.n + 4; } \
             return c.n - 7; }",
    );
}

#[test]
fn a_shared_reference_may_not_be_written_through() {
    let message = refused(
        "struct C { n: int } fn main() -> [] int { var c = C { n: 1 }; \
             borrow c as &r in { r.n = 2; } return 0; }",
    );
    assert!(message.contains("shared reference"), "{message}");
}

#[test]
fn a_field_of_an_owned_local_is_not_a_place() {
    let message = refused(
        "struct C { n: int } fn main() -> [] int { var c = C { n: 1 }; c.n = 2; return 0; }",
    );
    assert!(message.contains("assign the whole value instead"), "{message}");
}

#[test]
fn a_write_through_a_reference_may_not_drop_a_res_field() {
    // Overwriting names no consumer for what was there, which is the
    // silent drop §4 refuses however it is spelled.
    let message = refused(
        "struct Holder { f: Ticket } \
             fn set[&r](h: &!r Holder) -> [] int { h.f = open(2); return 0; } \
             fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("cannot be discarded"), "{message}");
}

#[test]
fn a_region_is_erased_and_does_not_copy_a_function() {
    // Regions have no runtime meaning, so a region-polymorphic function
    // is emitted once however many regions call it. A *type* parameter
    // still copies.
    let program = check(
        "fn size[&p](h: &p Ticket) -> [] int { return h.fd; } \
             fn main() -> [] int { let f = open(1); \
             borrow f as &a in { let x = size(a); } \
             borrow f as &b in { let y = size(b); } return close(f); }",
    )
    .expect("accepted");
    let copies = program.funcs.iter().filter(|f| f.name.starts_with("size")).count();
    assert_eq!(copies, 1, "{:?}", program.funcs.iter().map(|f| &f.name).collect::<Vec<_>>());
}

#[test]
fn a_path_prefix_extends_at_a_separator() {
    // `docs/filesystem.md` §1.1. The distinction a byte comparison
    // cannot make, and the one a filesystem cares about.
    assert!(crate::extends_path("/tmp", "/tmp/a"));
    assert!(crate::extends_path("/tmp", "/tmp"), "a capability may name one file");
    assert!(crate::extends_path("/tmp/", "/tmp/a"), "the prefix already ends at a boundary");
    assert!(crate::extends_path("", "/anywhere"), "the root contains everything");

    assert!(!crate::extends_path("/tmp", "/tmpevil"), "the directory next door");
    assert!(!crate::extends_path("/tmp/a", "/tmp"), "widening is the one direction there is not");
    assert!(!crate::extends_path("/tmp", "/var/log"));
}

#[test]
fn a_filesystem_capability_narrows_like_a_foreign_one() {
    // `Fs` is the second capability carrying a value, over the same
    // `narrow`: the type records where it may reach, and two different
    // prefixes are two different types.
    let program = check(
            "fn main(world: World) -> [] int {              let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(ffi); release(io);              let tmp = narrow(fs, \"/tmp\"); let app = narrow(tmp, \"/tmp/app\");              return release(app); }",
        )
        .expect("accepted");
    assert!(program.funcs.iter().any(|f| f.name == "main"));
}

#[test]
fn a_filesystem_capability_cannot_step_sideways() {
    let message = refused(
        "fn main(world: World) -> [] int {              let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(ffi); release(io);              let tmp = narrow(fs, \"/tmp\"); let evil = narrow(tmp, \"/tmpevil\");              return release(evil); }",
    );
    assert!(message.contains("a path prefix extends at a `/`"), "{message}");
}

#[test]
fn a_file_operation_needs_the_capability_that_names_a_path() {
    // §2: the authority that guards the filesystem has to be the one
    // that names the filesystem, which is why the operations are
    // builtins rather than `extern fn` gated by `Ffi("libc")`.
    let message = refused(
        "fn main(world: World) -> [] int {              let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(ffi); release(fs);              var n = 0; region a { let b = alloc_slice[a](4, byte_of(0));              borrow mut io as &!i in { n = fs_read(i, \"/tmp/x\", b); } }              release(io); return n; }",
    );
    assert!(message.contains("is not a borrowed `Fs`"), "{message}");
}

#[test]
fn a_file_operations_row_carries_the_prefix() {
    // §1: the label is `fs_read("/tmp")`, not `fs_read`. A row that
    // dropped the prefix would take away the thing the prefix is for.
    let message = refused(
        "fn peek[&f, &b](fs: &f Fs(\"/tmp\"), into: &!b [byte]) -> [] int {              return fs_read(fs, \"/tmp/x\", into); }              fn main(world: World) -> [] int { let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); \
             release(ffi); release(fs); return release(io); }",
    );
    assert!(message.contains("fs_read(\"/tmp\")"), "{message}");
}

#[test]
fn a_type_may_contain_itself_through_a_box() {
    // `docs/heap.md` §4: the one hole in the size check, and the whole
    // reason the heap exists. A box is a pointer however large what it
    // points at is, so the size computation terminates.
    let program = check(
        "enum List { Empty, Cons(int, Box[List]) } \
             fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); release(args); \
             release(ffi); release(fs); release(io); release(heap); return 0; }",
    )
    .expect("accepted");
    assert!(program.funcs.iter().any(|f| f.name == "main"));
}

#[test]
fn a_type_may_not_contain_itself_without_one() {
    let message = refused("enum List { Empty, Cons(int, List) } fn main() -> [] int { return 0; }");
    assert!(message.contains("contains itself"), "{message}");
    assert!(message.contains("put a `Box`"), "{message}");
}

#[test]
fn a_type_may_not_contain_itself_through_a_type_argument() {
    // The hole this slice closed. `reaches` followed member types but
    // not their *arguments*, so this was accepted although it has no
    // more finite a size than writing `Wrap`'s field out: no program
    // could build one, because the checker refused every attempt at a
    // value, but the refusal landed at each use rather than here.
    let message = refused(
        "struct Wrap[T] { t: T } struct Node { w: Wrap[Node] } \
             fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("contains itself"), "{message}");
}

#[test]
fn a_box_is_one_leaf_however_large_what_it_holds() {
    // §3.2: no header, no refcount, no tag. This is what makes
    // `contents` a load and §4's hole sound.
    let boxed = crate::Type::Named(crate::DefId(crate::PRELUDE_BOX as u32), vec![crate::Type::Int]);
    assert!(!crate::leaf_free(&boxed), "a box is a pointer, so it is not free to thread");
}

#[test]
fn owning_a_heap_discharges_the_allocation_effect() {
    // §8.2's rule, unchanged: owning authority is stronger than
    // borrowing it, so `main`'s row stays `[]` while the program
    // allocates.
    let program = check(
        "fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); release(args); \
             release(ffi); release(fs); release(io); \
             var n = 0; \
             borrow mut heap as &!h in { let b = box(h, 41); n = unbox(h, b); } \
             release(heap); return n - 41; }",
    )
    .expect("accepted");
    let main = program.funcs.iter().find(|f| f.name == "main").expect("a main");
    assert!(main.effects.is_pure(), "`main` should declare [], not {:?}", main.effects);
}

#[test]
fn allocating_through_a_borrowed_heap_declares_the_effect() {
    let message = refused(
        "fn stash[&h](heap: &!h Heap, n: int) -> [] Box[int] { return box(heap, n); } \
             fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("performs `heap`"), "{message}");
}

#[test]
fn contents_needs_a_borrowed_box() {
    let message = refused(
        "struct Point { x: int } \
             fn peek[&r](p: &r Point) -> [] int { return contents(p).x; } \
             fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("is not a borrowed `Box`"), "{message}");
}

#[test]
fn matching_a_reference_binds_references() {
    // `docs/reading-references.md` §2: the whole type rule. Nothing
    // moves out of a reference, so a `res` payload binds as a borrow
    // and the scrutinee is as owned after the match as before it.
    let message = refused(
        "enum Holder { None, Some(Ticket) } \
             fn peek[&r](h: &r Holder) -> [] int { \
             match h { Holder::None => { return 0; } \
             Holder::Some(f) => { return close(f); } } } \
             fn main() -> [] int { return 0; }",
    );
    // `close` takes a `Ticket`; this is a `&r Ticket`.
    assert!(message.contains("expected `Ticket`"), "{message}");
}

#[test]
fn matching_a_reference_consumes_nothing() {
    // The counterpart: the same enum read through a reference twice,
    // then consumed once. Before this rule the first read *was* the
    // consumption and the second would not compile.
    assert!(
        check(
            "enum Holder { None, Some(Ticket) } \
                 fn tag[&r](h: &r Holder) -> [] int { \
                 match h { Holder::None => { return 0; } Holder::Some(_) => { return 1; } } } \
                 fn main() -> [] int { let h = Holder::Some(open(1)); var n = 0; \
                 borrow h as &r in { n = tag(r) + tag(r); } \
                 match h { Holder::None => { return n; } \
                 Holder::Some(f) => { return n + close(f); } } }",
        )
        .is_ok()
    );
}

#[test]
fn a_wildcard_through_a_reference_drops_nothing() {
    // `_` on an owned `res` enum is the silent drop §4 forbids. Through
    // a reference there is nothing to drop, because the match never
    // owned it.
    assert!(
        check(
            "enum Holder { None, Some(Ticket) } \
                 fn tag[&r](h: &r Holder) -> [] int { match h { _ => { return 1; } } } \
                 fn main() -> [] int { return 0; }",
        )
        .is_ok()
    );
    let message = refused(
        "enum Holder { None, Some(Ticket) } \
             fn take(h: Holder) -> [] int { match h { _ => { return 1; } } } \
             fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("cannot be discarded"), "{message}");
}

#[test]
fn a_deref_copies_only_a_val() {
    // §3: copying a `res` out of a reference would leave two values
    // where one obligation is owed.
    let message = refused(
        "fn peek[&r](f: &r Ticket) -> [] Ticket { return *f; } \
             fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("`*` would copy it"), "{message}");
}

#[test]
fn a_deref_needs_a_reference() {
    let message =
        refused("fn f() -> [] int { let n = 1; return *n; } fn main() -> [] int { return 0; }");
    assert!(message.contains("is not a reference"), "{message}");
}

#[test]
fn writing_through_a_deref_needs_a_unique_reference() {
    let message = refused(
        "fn set[&r](n: &r int) -> [] int { *n = 9; return 0; } \
             fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("shared reference"), "{message}");
}

#[test]
fn reading_the_command_line_is_an_effect() {
    // `docs/arguments.md` §2, and the reason `Args` is a capability at
    // all: not that argv is dangerous, but that a function whose
    // behaviour depends on it should say so.
    let message = refused(
        "fn verbose[&a](args: &a Args) -> [] bool { return arg_count(args) > 1; } \
             fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("performs `args`"), "{message}");
}

#[test]
fn an_argument_is_a_shared_static_slice() {
    // §3.1: `static` because argv outlives every region, shared
    // because a program does not own its own command line.
    let program = check(
        "fn first[&a](args: &a Args) -> [args] int { return len(arg(args, 0)); } \
             fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(ffi); release(fs); release(heap); release(io); \
             var n = 0; borrow args as &a in { n = first(a); } \
             release(args); return n - n; }",
    )
    .expect("accepted");
    assert!(program.funcs.iter().any(|f| f.name == "main"));
}

#[test]
fn owning_args_discharges_reading_them() {
    // §8.2's rule again: `main` owns the capability, so its row stays
    // `[]` while the program reads the command line.
    let program = check(
        "fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(ffi); release(fs); release(heap); release(io); \
             var n = 0; borrow args as &a in { n = arg_count(a); } \
             release(args); return n - 1; }",
    )
    .expect("accepted");
    let main = program.funcs.iter().find(|f| f.name == "main").expect("a main");
    assert!(main.effects.is_pure(), "`main` should declare [], not {:?}", main.effects);
}

#[test]
fn every_capability_taking_builtin_counts_its_region() {
    // A builtin whose signature mentions `Region::Param(n)` must say so
    // in `regions()`, or the parameter stays *rigid* and the call works
    // from a region-polymorphic function while failing from inside a
    // `borrow` block. That is a confusing way to find out, so it is
    // checked here instead.
    for builtin in crate::Builtin::ALL {
        let prelude: Vec<crate::DefId> =
            (0..crate::PRELUDE_COUNT as u32).map(crate::DefId).collect();
        let (params, ret) = builtin.signature(&prelude);
        let highest = params
            .iter()
            .chain(std::iter::once(&ret))
            .filter_map(|t| match t {
                crate::Type::Ref { region: crate::Region::Param(i), .. } => Some(*i + 1),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        assert!(
            builtin.regions() >= highest as usize,
            "`{}` names {highest} region parameter(s) but declares {}",
            builtin.name(),
            builtin.regions()
        );
    }
}

#[test]
fn a_res_field_through_a_reference_is_a_borrow() {
    // `docs/reading-references.md` §2.0. A `res` field cannot be
    // copied, so what comes back is a reference to it -- exactly as
    // `match` on a reference binds a `res` payload.
    accepted(
        "res struct Holder { f: Ticket } \
             fn peek[&r](h: &r Holder) -> [] int { let taken = h.f; return 0; } \
             fn main() -> [] int { return 0; }",
    );

    // And consuming it is refused, which is the soundness hole this
    // has now closed twice: allowed once (a double free under
    // valgrind), then refused outright, which overshot. The refusal
    // belongs at the *use*, because the double free was never about
    // reading -- and the ordinary type rule already puts it there.
    let message = refused(
        "res struct Holder { f: Ticket } \
             fn steal[&r](h: &r Holder) -> [] int { let taken = h.f; return close(taken); } \
             fn main() -> [] int { return 0; }",
    );
    assert!(message.contains("expected `Ticket`, found `&r Ticket`"), "{message}");
}

#[test]
fn a_borrowed_field_carries_the_bases_mode_and_region() {
    // A field of a `&!r` is reachable uniquely, which is §2.2's
    // disjointness argument for payloads applied to fields: they are
    // different offsets in one value and no two names reach the same
    // one.
    accepted(
        "res struct Holder { f: Ticket } \
             fn peek[&r](h: &!r Holder) -> [] int { let taken = h.f; return 0; } \
             fn main() -> [] int { return 0; }",
    );

    // And it may be handed back at the function's own region, which
    // is exactly `contents`' shape: the caller supplied `r`, so a
    // reference valid for `r` is a reference the caller can hold.
    // Nothing here needed a new rule -- a block region still cannot
    // escape its block, by the same occurs-check as every other
    // reference (`contents_escapes_its_borrow.ls`).
    accepted(
        "res struct Holder { f: Ticket } \
             fn take[&r](h: &r Holder) -> [] &r Ticket { return h.f; } \
             fn main() -> [] int { return 0; }",
    );
}

#[test]
fn a_val_field_is_still_read_through_a_reference() {
    // The other half: copying a `val` field costs the referent
    // nothing, which is what a reference is for.
    assert!(
        check(
            "struct Point { x: int, y: int } \
                 fn sum[&r](p: &r Point) -> [] int { return p.x + p.y; } \
                 fn main() -> [] int { return 0; }",
        )
        .is_ok()
    );
}

#[test]
fn a_boxed_slice_holds_val_elements() {
    // `docs/boxed-slices.md` §2.1: the arena's rule, in the second
    // place it applies.
    let message = refused(
        "fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(fs); release(ffi); release(io); \
             var n = 0; \
             borrow mut heap as &!h in { let b = box_slice(h, 2, open(1)); n = unbox_slice(h, b); } \
             release(heap); return n; }",
    );
    assert!(message.contains("a boxed slice holds `val` data only"), "{message}");
}

#[test]
fn unbox_refuses_an_unsized_referent() {
    // §3: `unbox` hands back what the box held, and `[T]` is unsized.
    // Without this the backend's own assertion fired instead, which is
    // a worse way to find out.
    let message = refused(
        "fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(fs); release(ffi); release(io); \
             var n = 0; \
             borrow mut heap as &!h in { let b = box_slice(h, 2, 0); let back = unbox(h, b); n = 0; } \
             release(heap); return n; }",
    );
    assert!(message.contains("`unbox` has nothing to hand back"), "{message}");
}

// ---- tuples (`docs/tuples.md`) -------------------------------------

/// `docs/tuples.md` §2.3: the mode is read off the components, because
/// there is nowhere to declare one.
///
/// The unit test rather than only a fixture, because this is the claim
/// the whole feature rests on -- a tuple with no `res` written anywhere
/// near it still carries every obligation `res` carries.
#[test]
fn a_tuple_is_res_exactly_when_a_component_is() {
    let leaked = refused(
        "fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(io); \
             borrow mut heap as &!h in { let pair = (box(h, 1), 2); } \
             release(heap); return 0; }",
    );
    assert!(leaked.contains("still live at the end"), "{leaked}");

    // And the same shape with no `res` component is an ordinary value:
    // nothing consumes it and nothing has to.
    accepted(
        "fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(heap); release(io); \
             let pair = (1, 2); return pair.0 + pair.1; }",
    );
}

/// §3.1, both reference cases in one test, because the rule is that
/// they are *one* rule seen from two sides.
///
/// A `val` component copies out of a reference and costs the referent
/// nothing. A `res` one cannot copy, so reading it would leave two
/// owners of one value -- `reading-references.md` §2, enforced for
/// tuples from the start rather than after a double free found it
/// missing, which is how it was found for struct fields.
#[test]
fn a_res_component_borrows_out_of_a_reference_and_a_val_one_copies() {
    // Binding it is a borrow, like a struct's field; consuming it is
    // refused by the type, like a struct's field. A tuple is an
    // anonymous struct, so it had better not need its own ideas.
    accepted(
        "fn peek[&p](pair: &p (Box[int], int)) -> [] int { let held = pair.0; return 0; } \
             fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(heap); release(io); return 0; }",
    );
    let message = refused(
        "fn steal[&h, &p](heap: &!h Heap, pair: &p (Box[int], int)) -> [heap] int { \
             return unbox(heap, pair.0); } \
             fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(heap); release(io); return 0; }",
    );
    assert!(message.contains("found `&p Box[int]`"), "{message}");

    accepted(
        "fn peek[&p](pair: &p (int, int)) -> [] int { return pair.0 + pair.1; } \
             fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(heap); release(io); return 0; }",
    );
}

/// §2.2: structural, and exactly structural. Arity is part of the
/// match, so there is no prefix rule and no reordering.
#[test]
fn two_tuples_are_the_same_type_when_their_components_are() {
    accepted(
        "fn id(pair: (int, bool)) -> [] (int, bool) { return pair; } \
             fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(heap); release(io); \
             let p = id((1, true)); return p.0; }",
    );

    let reordered = refused(
        "fn id(pair: (int, bool)) -> [] (int, bool) { return pair; } \
             fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(heap); release(io); \
             let p = id((true, 1)); return p.0; }",
    );
    assert!(reordered.contains("expected `int`, found `bool`"), "{reordered}");

    let longer = refused(
        "fn id(pair: (int, bool)) -> [] (int, bool) { return pair; } \
             fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(heap); release(io); \
             let p = id((1, true, 1)); return p.0; }",
    );
    assert!(longer.contains("expected"), "{longer}");
}

/// §2: a tuple is an aggregate with no declaration, so the acyclicity
/// walk has to see through it. `struct Node { pair: (int, Node) }` has
/// no more finite a size than `struct Node { next: Node }`.
///
/// The walk follows member types and their *type arguments*; a tuple is
/// neither, so it needed a case of its own and would have been a hole
/// without one.
#[test]
fn a_type_cannot_contain_itself_through_a_tuple() {
    let message = refused("struct Node { pair: (int, Node) } fn main() -> [] int { return 0; }");
    assert!(message.contains("contains itself"), "{message}");

    // Through a `Box`, it terminates -- the hole `heap.md` §4 opened on
    // purpose, and a tuple does not close it by accident.
    accepted("struct Node { pair: (int, Box[Node]) } fn main() -> [] int { return 0; }");
}

/// §3.2: a pattern is not an annotation.
///
/// The arity comes from the value, so taking apart something that is
/// not a tuple says so rather than solving an inference variable from
/// the pattern and inventing a type nobody wrote.
#[test]
fn a_tuple_pattern_is_checked_against_the_value_not_the_other_way() {
    let message = refused(
        "fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(heap); release(io); \
             let (a, b) = 1; return a; }",
    );
    assert!(message.contains("is not a tuple"), "{message}");
}

// ---- shadowing (`docs/shadowing.md`) -------------------------------

/// §3: shadowing is allowed exactly when the shadowed binding is dead.
///
/// Both halves in one test, because the rule is one rule: a `val`
/// binding never owed anything so it shadows freely, and a `res` one
/// shadows only after something consumed it.
#[test]
fn a_binding_may_be_shadowed_exactly_when_it_is_dead() {
    accepted(
        "fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(heap); release(io); \
             let n = 4; let n = n * 10; let n = n + 2; return n; }",
    );

    // Consumed first -- `unbox` takes the old `held`, and a `let`'s
    // initialiser is lowered before the binding exists, so the old one
    // is dead by the time the new one is declared.
    accepted(
        "fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(io); \
             var n = 0; \
             borrow mut heap as &!h in { let held = box(h, 1); let held = unbox(h, held); n = held; } \
             release(heap); return n; }",
    );

    let leaked = refused(
        "fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(io); \
             var n = 0; \
             borrow mut heap as &!h in { let held = box(h, 1); let held = box(h, 2); n = unbox(h, held); } \
             release(heap); return n; }",
    );
    assert!(leaked.contains("shadowing it here would put that value out of reach"), "{leaked}");
}

/// §2.1: an *inner* block shadows a live binding freely, and always
/// did.
///
/// The inner block ends first, so the outer binding is reachable again
/// afterwards and its own block-close check still covers it. That is
/// the distinction the rule turns on -- not whether a name is reused,
/// but whether reusing it strands an obligation.
#[test]
fn an_inner_block_may_shadow_a_binding_that_is_still_live() {
    accepted(
        "fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(io); \
             var n = 0; \
             borrow mut heap as &!h in { \
               let held = box(h, 1); \
               if n == 0 { let held = 7; n = held; } \
               n = unbox(h, held); \
             } \
             release(heap); return n; }",
    );
}

/// §4.1: a parameter is the one place two scopes are really one.
///
/// The parameter list has no statements of its own and closes with the
/// body's top-level block, so a `let` at the top of a body covers a
/// parameter for the whole of that parameter's life. This program was
/// always refused -- the value stayed live to the `return` -- but the
/// message named a binding the reported line does not mention.
#[test]
fn shadowing_a_live_parameter_is_reported_at_the_let() {
    let source = "res struct Ticket { serial: int } \
             fn redeem(t: Ticket) -> [] int { let t = 1; return t; } \
             fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(heap); release(io); \
             return redeem(Ticket { serial: 1 }); }";
    let error = lower_src(source).expect_err("refused");
    assert!(
        error.message.contains("shadowing it here would put that value out of reach"),
        "{}",
        error.message
    );
    // And it points at the `let`, not at the `return` three tokens on.
    assert!(
        source[error.span.start as usize..error.span.end as usize].starts_with("let t"),
        "reported at `{}`",
        &source[error.span.start as usize..error.span.end as usize]
    );

    // A `val` parameter shadows freely, which it always did.
    accepted("fn f(n: int) -> [] int { let n = n + 1; return n; }");
}

/// §5: shadowing is between statements, never within one pattern.
///
/// The check that refuses a name bound twice in one pattern is a
/// different check from the one this slice relaxed, and it stays:
/// `let Pair { a, a }` names one field twice, which is not shadowing
/// but a pattern that does not take the whole value apart.
#[test]
fn a_pattern_still_binds_each_name_once() {
    let message = refused(
        "fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(heap); release(io); \
             let (a, a) = (1, 2); return a; }",
    );
    assert!(message.contains("bound twice in this pattern"), "{message}");
}

// ---- standard input (`docs/standard-input.md`) ---------------------

/// §2.1: the two labels are a distinction, not a spelling.
///
/// This is the test the whole rename exists for. If a row saying
/// `[io_write]` let a function read, a caller reading that row would
/// learn nothing about whether its input is being consumed -- and the
/// row would be decoration, which §7.3 says it must never be.
#[test]
fn a_write_row_does_not_permit_a_read_or_the_other_way() {
    let reading = refused(
        "fn sink[&i](io: &!i Io) -> [io_write] int { return getchar(io); } \
             fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(heap); release(io); return 0; }",
    );
    assert!(reading.contains("performs `io_read`"), "{reading}");
    assert!(reading.contains("[io_write]"), "the row it had is named: {reading}");

    let writing = refused(
        "fn source[&i](io: &!i Io) -> [io_read] int { return putchar(io, 65); } \
             fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(heap); release(io); return 0; }",
    );
    assert!(writing.contains("performs `io_write`"), "{writing}");

    // Both declared, both performed: exact, which is what §7.3 wants.
    accepted(
        "fn both[&i](io: &!i Io) -> [io_read, io_write] int { \
             let c = getchar(io); return putchar(io, c); } \
             fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(heap); \
             var n = 0; borrow mut io as &!i in { n = both(i); } release(io); return 0; }",
    );
}

/// §2.2: owning an `Io` outright discharges *both* labels.
///
/// "Owning discharges, borrowing declares" did not change, and adding
/// a second label to a capability is the first time that rule had more
/// than one label to discharge for anything but `Fs`. `main` owns the
/// `Io`, reads and writes through it, and its row is still `[]`.
#[test]
fn owning_the_console_discharges_both_directions() {
    let program = lower_src(
        "fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(heap); \
             var c = 0; \
             borrow mut io as &!i in { c = getchar(i); putchar(i, c); } \
             release(io); return 0; }",
    )
    .expect("this should be accepted");
    let main = program.func(program.find("main").expect("main"));
    assert!(main.effects.is_pure(), "owning discharges: {}", main.effects);
}

/// §3: `getchar` counts its region parameter.
///
/// A builtin that forgets to keeps `Region::Param(0)` rigid, and then
/// the call works from a region-polymorphic function and fails inside
/// a `borrow` block -- which is how it was found the last time, and is
/// why `every_capability_taking_builtin_counts_its_region` exists.
/// This is the same claim for the newest one, exercised both ways.
#[test]
fn getchar_works_from_a_borrow_block_and_from_a_polymorphic_function() {
    accepted(
        "fn peek[&i](io: &!i Io) -> [io_read] int { return getchar(io); } \
             fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(heap); \
             var c = 0; \
             borrow mut io as &!i in { c = getchar(i) + peek(i); } \
             release(io); return 0; }",
    );
}

// ---- modules (`docs/modules.md`) -----------------------------------

/// §3: a name is scoped to its module, so two modules may declare one.
///
/// In one flat namespace this was "declared twice". It still is
/// *within* a module -- and the root is a module, which is why every
/// program written before this is unaffected.
#[test]
fn two_modules_may_declare_the_same_name() {
    let mut ast = lex_sys_syntax::ast::Ast::new();
    lex_sys_syntax::parse_into(
            &mut ast,
            "module a; pub struct Buffer { n: int } pub fn make() -> [] Buffer { return Buffer { n: 1 }; }",
            0,
        )
        .expect("parses");
    lex_sys_syntax::parse_into(
            &mut ast,
            "module b; pub struct Buffer { n: int } pub fn make() -> [] Buffer { return Buffer { n: 2 }; }",
            2000,
        )
        .expect("parses");
    lex_sys_syntax::parse_into(
        &mut ast,
        "import a; import b; fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(heap); release(io); \
             let x: a.Buffer = a.make(); let y: b.Buffer = b.make(); return x.n + y.n - 3; }",
        4000,
    )
    .expect("parses");
    crate::lower(&ast).expect("two modules, one name, no clash");
}

/// §6, the one that matters: a module is **not** a trust boundary.
///
/// `pub` means reachable, never safe. A `pub fn` that writes to the
/// console still needs a caller holding an `Io`, and its row still
/// says `io_write` -- being public buys it nothing, and a caller that
/// declares `[]` is refused exactly as it would be within one module.
///
/// Worth a test rather than a sentence, because "public API" means
/// "sanctioned" in most languages and here it must not.
#[test]
fn pub_grants_no_authority_and_hides_no_effect() {
    let mut ast = lex_sys_syntax::ast::Ast::new();
    lex_sys_syntax::parse_into(
        &mut ast,
        "module lib; pub fn shout[&i](io: &!i Io) -> [io_write] int { return putchar(io, 33); }",
        0,
    )
    .expect("parses");
    lex_sys_syntax::parse_into(
        &mut ast,
        "import lib; \
             fn quiet[&i](io: &!i Io) -> [] int { return lib.shout(io); } \
             fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(heap); release(io); return 0; }",
        2000,
    )
    .expect("parses");
    let message = crate::lower(&ast).expect_err("the row is still exact").message;
    assert!(message.contains("performs `io_write`"), "{message}");
}

/// §4.2: a module may import one that imports it back.
///
/// In most languages a cycle is a problem because imports drive load
/// order. Here the program is the set of files on the command line,
/// compiled at once, and an import is a rule for resolving a name --
/// so there is no order to be circular.
#[test]
fn modules_may_import_each_other() {
    let mut ast = lex_sys_syntax::ast::Ast::new();
    lex_sys_syntax::parse_into(
        &mut ast,
        "module a; import b; pub fn one() -> [] int { return 1; } \
             pub fn three() -> [] int { return one() + b.two(); }",
        0,
    )
    .expect("parses");
    lex_sys_syntax::parse_into(
        &mut ast,
        "module b; import a; pub fn two() -> [] int { return a.one() + a.one(); }",
        2000,
    )
    .expect("parses");
    lex_sys_syntax::parse_into(
        &mut ast,
        "import a; fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(heap); release(io); \
             return a.three() - 3; }",
        4000,
    )
    .expect("parses");
    crate::lower(&ast).expect("a cycle is not a problem here");
}

/// §5.1: the root has no name, so nothing can import it.
///
/// The root sees out and nothing sees in. A module reaching a
/// root-module function fails the same way any unresolved name does,
/// because there is no qualifier that could name the root.
#[test]
fn a_module_cannot_reach_into_the_root() {
    let mut ast = lex_sys_syntax::ast::Ast::new();
    lex_sys_syntax::parse_into(&mut ast, "fn helper() -> [] int { return 7; }", 0).expect("parses");
    lex_sys_syntax::parse_into(
        &mut ast,
        "module lib; pub fn use_it() -> [] int { return helper(); }",
        2000,
    )
    .expect("parses");
    let message = crate::lower(&ast).expect_err("the root is not reachable").message;
    assert!(message.contains("`helper` is not a function"), "{message}");
}

// ---- mode polymorphism (`docs/mode-polymorphism.md`) ---------------

/// §2: a declared `val` on a generic was believed rather than checked.
///
/// `Wrap[Box[int]]` came out `val` by assertion, which is a leak (the
/// box need never be unboxed) and, because `val` means copyable, a
/// double free. Both compiled; valgrind reported *8 bytes definitely
/// lost* and *Invalid free()* respectively.
///
/// The declaration-time check could not catch it, because it runs
/// against the members **as written**, where `T` is a parameter
/// rather than `Box[int]`.
#[test]
fn a_val_generic_at_a_res_argument_is_not_val() {
    const PRE: &str = "val struct Wrap[T] { held: T } \
             fn main(world: World) -> [] int { \
             let Split { io, ffi, fs, heap, args } = split(world); \
             release(args); release(ffi); release(fs); release(io); var n = 0; \
             borrow mut heap as &!h in { ";
    const POST: &str = " } release(heap); return n; }";

    // The leak: nothing consumes it, and nothing had to.
    let leak = refused(&format!("{PRE} let w = Wrap {{ held: box(h, 1) }}; n = 0;{POST}"));
    assert!(leak.contains("still live"), "{leak}");

    // The double free: `val` copies, so two owners of one allocation.
    let copied = refused(&format!(
        "{PRE} let w = Wrap {{ held: box(h, 1) }}; let a = w; let b = w; \
             let Wrap {{ held }} = a; let Wrap {{ held }} = b; n = 0;{POST}"
    ));
    assert!(copied.contains("already been consumed"), "{copied}");

    // And a written instantiation says so at the type, which is the
    // clearer place when there is one (§3).
    let written = refused(
        "val struct Wrap[T] { held: T } \
             fn f(w: Wrap[Box[int]]) -> [] int { return 0; } \
             fn main() -> [] int { return 0; }",
    );
    assert!(written.contains("is declared `val`, so its type arguments"), "{written}");

    // A `val` generic at a `val` argument is still fine, which is the
    // whole point of being able to declare one.
    accepted(
        "val struct Wrap[T] { held: T } fn main() -> [] int { \
                  let w = Wrap { held: 1 }; return w.held - 1; }",
    );
}

/// A call resolved **once**.
///
/// Its effect row and its types came from two independent lookups
/// with different precedence, neither scoped to a module. Once
/// `docs/modules.md` let two modules hold one name they could
/// disagree: a root `extern fn puts` called from the root took its
/// types from the extern and its effects from an unrelated `puts`
/// somewhere else — so a function calling into C declared `[]` and
/// compiled.
///
/// The effect system is the whole point of the language, so this is
/// the most serious kind of bug it can have: not a crash, a *lie*.
#[test]
fn a_call_takes_its_effects_from_the_callee_it_resolves_to() {
    let mut ast = lex_sys_syntax::ast::Ast::new();
    lex_sys_syntax::parse_into(
        &mut ast,
        "module quiet; pub fn puts(text: int) -> [] int { return text; }",
        0,
    )
    .expect("parses");
    lex_sys_syntax::parse_into(
        &mut ast,
        "extern fn puts[&f](ffi: &f Ffi(\"libc\"), s: &static [byte]) -> [ffi(\"libc\")] int; \
             fn sneak[&f](libc: &f Ffi(\"libc\")) -> [] int { return puts(libc, \"x\"); } \
             fn main() -> [] int { return 0; }",
        2000,
    )
    .expect("parses");
    let message = crate::lower(&ast).expect_err("the row is a lie").message;
    assert!(message.contains("performs `ffi(\"libc\")`"), "{message}");
}

#[test]
fn the_linearity_check_says_where() {
    // Every rule here reports at a span the programmer wrote, not at the
    // enclosing function: the trace carries spans precisely so it can.
    let source = format!("{PRELUDE}fn main() -> [] int {{ let f = open(1); return 0; }}");
    let error = lower_src(&source).expect_err("refused");
    assert!(source[error.span.start as usize..error.span.end as usize].starts_with("return"));
}
