use super::*;
use lex_sys_syntax::parse;

pub(super) fn lower_src(src: &str) -> Result<Program, Diagnostic> {
    lower(&parse(src).expect("should parse"))
}

fn error(src: &str) -> String {
    lower_src(src).expect_err("should be refused").message
}

fn main_fn(src: &str) -> Func {
    lower_src(src).expect("should check").funcs.pop().expect("a function")
}

// ---- resolution ----------------------------------------------------

#[test]
fn parameters_take_the_first_slots() {
    let f = main_fn("fn f(a: int, b: int) -> [] int { let c = a + b; return c; }");
    assert_eq!(f.n_params, 2);
    assert_eq!(f.n_slots(), 3);
    assert_eq!(
        f.body[0],
        Stmt::Store {
            place: Place::Slot(Slot(2)),
            value: Expr::Bin {
                op: BinOp::Add,
                lhs: Box::new(Expr::Load(Slot(0))),
                rhs: Box::new(Expr::Load(Slot(1))),
            }
        }
    );
}

#[test]
fn functions_are_visible_before_they_are_defined() {
    let p = lower_src("fn f() -> [] int { return g(); } fn g() -> [] int { return 1; }").unwrap();
    assert_eq!(p.funcs.len(), 2);
    assert_eq!(p.find("g"), Some(FuncId(1)));
}

#[test]
fn an_inner_block_may_shadow() {
    let f = main_fn(
        "fn main[&i](io: &!i Io) -> [io_write] int { \
             let x = 1; if true { let x = 2; putchar(io, x); } return x; }",
    );
    // The capability, `x`, and the shadowing `x`.
    assert_eq!(f.n_slots(), 3);
}

/// `docs/shadowing.md` §3, from the resolution side: a shadow is a
/// *second binding*, not a reassignment.
///
/// Each `let` gets its own slot, and a name resolves to the most
/// recent one. That is what makes shadowing at a different type work,
/// and what makes the old binding something the checker can still
/// have an opinion about rather than something that has been
/// overwritten.
#[test]
fn rebinding_in_the_same_block_declares_a_second_slot() {
    let f = main_fn("fn f() -> [] int { let x = 1; let x = true; return 2; }");
    assert_eq!(f.n_slots(), 2, "two bindings, two slots");
    assert_eq!(f.slots[0], Type::Int);
    assert_eq!(f.slots[1], Type::Bool);
}

#[test]
fn an_initialiser_cannot_see_its_own_binding() {
    assert!(error("fn f() -> [] int { let x = x; return x; }").contains("not bound"));
}

#[test]
fn assigning_to_a_let_is_refused() {
    assert!(error("fn f() -> [] int { let x = 1; x = 2; return x; }").contains("immutable"));
}

#[test]
fn assigning_to_a_parameter_is_refused() {
    assert!(error("fn f(a: int) -> [] int { a = 1; return a; }").contains("immutable"));
}

#[test]
fn assigning_to_a_var_is_allowed() {
    assert!(lower_src("fn f() -> [] int { var x = 1; x = 2; return x; }").is_ok());
}

#[test]
fn unknown_names_are_refused() {
    assert!(error("fn f() -> [] int { return nope; }").contains("not bound"));
    assert!(error("fn f() -> [] int { return nope(); }").contains("not a function"));
}

#[test]
fn arity_is_checked_for_functions_and_builtins() {
    assert!(
        error("fn g(a: int) -> [] int { return a; } fn f() -> [] int { return g(); }")
            .contains("takes 1 argument")
    );
    assert!(
        error("fn f[&i](io: &!i Io) -> [] int { return putchar(io); }")
            .contains("takes 2 arguments")
    );
}

#[test]
fn a_function_is_not_a_value() {
    assert!(
        error("fn g() -> [] int { return 1; } fn f() -> [] int { return g; }")
            .contains("no function values")
    );
}

#[test]
fn a_local_is_not_callable() {
    assert!(error("fn f() -> [] int { let g = 1; return g(); }").contains("not a function"));
}

#[test]
fn duplicate_definitions_are_refused() {
    assert!(
        error("fn f() -> [] int { return 1; } fn f() -> [] int { return 2; }")
            .contains("defined twice")
    );
    assert!(error("fn f(a: int, a: int) -> [] int { return a; }").contains("bound twice"));
}

#[test]
fn builtins_cannot_be_redefined() {
    assert!(error("fn putchar(c: int) -> [] int { return c; }").contains("builtin"));
}

// ---- control flow --------------------------------------------------

#[test]
fn every_path_must_return() {
    assert!(error("fn f() -> [] int { let x = 1; }").contains("without returning"));
    assert!(error("fn f() -> [] int { if true { return 1; } }").contains("without returning"));
    assert!(lower_src("fn f() -> [] int { if true { return 1; } else { return 2; } }").is_ok());
    // Conservative on purpose: a loop never counts as a terminator.
    assert!(error("fn f() -> [] int { while true { } }").contains("without returning"));
}

#[test]
fn code_after_a_return_is_refused() {
    assert!(error("fn f() -> [] int { return 1; return 2; }").contains("unreachable"));
}

// ---- types ---------------------------------------------------------

#[test]
fn slot_types_are_recorded_for_the_backend() {
    let f = main_fn("fn f(a: int, b: bool) -> [] int { let c = b; let d = a; return d; }");
    assert_eq!(f.slots, vec![Type::Int, Type::Bool, Type::Bool, Type::Int]);
    assert_eq!(f.ret, Type::Int);
}

#[test]
fn a_let_takes_its_type_from_its_initialiser() {
    let f = main_fn("fn f() -> [] bool { let x = 1 < 2; return x; }");
    assert_eq!(f.slots, vec![Type::Bool]);
}

#[test]
fn an_annotation_must_agree_with_the_initialiser() {
    assert!(
        error("fn f() -> [] int { let x: int = true; return x; }")
            .contains("expected `int`, found `bool`")
    );
    assert!(lower_src("fn f() -> [] bool { let x: bool = true; return x; }").is_ok());
}

#[test]
fn a_condition_must_be_a_bool() {
    assert!(
        error("fn f() -> [] int { if 1 { return 0; } return 1; }")
            .contains("expected `bool`, found `int`")
    );
    assert!(
        error("fn f() -> [] int { while 1 { } return 1; }")
            .contains("expected `bool`, found `int`")
    );
}

#[test]
fn return_must_match_the_signature() {
    assert!(error("fn f() -> [] int { return true; }").contains("expected `int`, found `bool`"));
    assert!(error("fn f() -> [] bool { return 1; }").contains("expected `bool`, found `int`"));
}

#[test]
fn an_argument_must_match_the_parameter() {
    assert!(
        error("fn f[&i](io: &!i Io) -> [io_write] int { return putchar(io, true); }")
            .contains("expected `int`, found `bool`")
    );
}

#[test]
fn an_assignment_must_match_the_binding() {
    assert!(
        error("fn f() -> [] int { var x = 1; x = true; return x; }")
            .contains("expected `int`, found `bool`")
    );
}

#[test]
fn arithmetic_is_for_numbers_and_logic_is_for_bools() {
    // Two numeric types now, so the refusal names both rather than
    // saying "expected `int`" (`docs/floating-point.md` §2).
    assert!(error("fn f() -> [] int { return true + true; }").contains("has no arithmetic"));
    assert!(error("fn f() -> [] bool { return 1 && 2; }").contains("expected `bool`"));
    assert!(error("fn f() -> [] int { return -true; }").contains("cannot be negated"));
    assert!(error("fn f() -> [] bool { return !1; }").contains("expected `bool`"));
}

#[test]
fn float_arithmetic_is_ieee754() {
    // `float` is arithmetic, and mixing it with `int` is not: there is
    // no implicit conversion in either direction (§4).
    assert_eq!(main_fn("fn f() -> [] float { return 1.5 + 2.0; }").ret, Type::Float);
    assert_eq!(main_fn("fn f() -> [] bool { return 1.5 < 2.0; }").ret, Type::Bool);
    assert!(error("fn f() -> [] float { return 1.5 + 2; }").contains("expected `float`"));
    assert!(error("fn f() -> [] int { return 1.5; }").contains("expected `int`"));
    // `%` is the one arithmetic operator a `float` does not get (§2).
    assert!(error("fn f() -> [] float { return 1.5 % 2.0; }").contains("expected `int`"));
    // And the conversions are spelled, both ways.
    assert_eq!(main_fn("fn f() -> [] float { return float_of(1); }").ret, Type::Float);
    assert_eq!(main_fn("fn f() -> [] int { return truncate(1.5); }").ret, Type::Int);
    assert_eq!(main_fn("fn f() -> [] bool { return is_nan(1.5); }").ret, Type::Bool);
}

#[test]
fn a_comparison_yields_a_bool() {
    let f = main_fn("fn f() -> [] bool { return 1 < 2; }");
    assert_eq!(f.ret, Type::Bool);
    // ...and ordering is for numbers only.
    assert!(error("fn f() -> [] bool { return true < false; }").contains("has no ordering"));
}

#[test]
fn equality_compares_two_values_of_the_same_type() {
    assert!(lower_src("fn f() -> [] bool { return 1 == 2; }").is_ok());
    assert!(lower_src("fn f() -> [] bool { return true == false; }").is_ok());
    assert!(error("fn f() -> [] bool { return 1 == true; }").contains("expected `int`"));
}

#[test]
fn unknown_types_are_refused_where_they_are_written() {
    assert!(error("fn f() -> [] i32 { return 0; }").contains("unknown type `i32`"));
    assert!(error("fn f(a: i32) -> [] int { return 0; }").contains("unknown type `i32`"));
    assert!(error("fn f() -> [] int { let x: i32 = 1; return x; }").contains("unknown type `i32`"));
}

#[test]
fn a_primitive_takes_no_type_arguments() {
    assert!(error("fn f() -> [] int[bool] { return 0; }").contains("takes no type arguments"));
}

// ---- structs -------------------------------------------------------

#[test]
fn a_struct_literal_is_positional_and_written_in_declaration_order() {
    // The IR holds the fields positionally, so the backend never has to
    // consult a field name.
    let f = main_fn("struct P { x: int, y: bool } fn f() -> [] P { return P { x: 7, y: true }; }");
    let Stmt::Return(Expr::Struct { fields, .. }) = &f.body[0] else { panic!("{:?}", f.body) };
    assert_eq!(fields[0], Expr::Int(7));
    assert_eq!(fields[1], Expr::Bool(true));

    // And writing them in another order is refused rather than silently
    // reordered: the fields run in declaration order, so any other
    // written order would hide which one runs first
    // (`docs/defined-behaviour.md`). This used to reorder quietly.
    let message = lower_src(
        "struct P { x: int, y: bool } fn f() -> [] P { return P { y: true, x: 7 }; } \
             fn main() -> [] int { return 0; }",
    )
    .expect_err("should be refused")
    .message;
    assert!(message.contains("declaration order"), "{message}");
}

#[test]
fn a_field_access_becomes_an_index() {
    let f = main_fn("struct P { x: int, y: int } fn f(p: P) -> [] int { return p.y; }");
    let Stmt::Return(Expr::Field { index, .. }) = &f.body[0] else { panic!() };
    assert_eq!(*index, 1);
}

#[test]
fn a_struct_literal_must_give_every_field_exactly_once() {
    let decl = "struct P { x: int, y: int } ";
    assert!(
        error(&format!("{decl}fn f() -> [] P {{ return P {{ x: 1 }}; }}"))
            .contains("missing field `y`")
    );
    assert!(
        error(&format!("{decl}fn f() -> [] P {{ return P {{ x: 1, y: 2, z: 3 }}; }}"))
            .contains("has no field `z`")
    );
    assert!(
        error(&format!("{decl}fn f() -> [] P {{ return P {{ x: 1, x: 2, y: 3 }}; }}"))
            .contains("given twice")
    );
    assert!(
        error(&format!("{decl}fn f() -> [] P {{ return P {{ x: true, y: 2 }}; }}"))
            .contains("expected `int`, found `bool`")
    );
}

#[test]
fn fields_are_checked_against_the_declaration() {
    assert!(
        error("struct P { x: int } fn f(p: P) -> [] int { return p.z; }")
            .contains("`P` has no field `z`")
    );
    assert!(error("fn f() -> [] int { let x = 1; return x.y; }").contains("`int` has no fields"));
}

#[test]
fn structs_may_nest_and_be_passed_by_value() {
    let p = lower_src(
        "struct P { x: int } struct L { a: P, b: P }              fn mid(l: L) -> [] int { return (l.a.x + l.b.x) / 2; }              fn f() -> [] int { return mid(L { a: P { x: 1 }, b: P { x: 3 } }); }",
    );
    assert!(p.is_ok(), "{:?}", p.err());
}

#[test]
fn a_struct_that_contains_itself_is_refused() {
    // No references in M1, so this has no finite size. Both the direct and
    // the mutual case, because the check is reachability rather than a
    // look at one field.
    assert!(
        error("struct N { next: N } fn f() -> [] int { return 0; }").contains("contains itself")
    );
    assert!(
        error("struct A { b: B } struct B { a: A } fn f() -> [] int { return 0; }")
            .contains("contains itself")
    );
    // ...but two fields of the same struct type are perfectly finite.
    assert!(
        lower_src("struct P { x: int } struct L { a: P, b: P } fn f() -> [] int { return 0; }")
            .is_ok()
    );
}

#[test]
fn struct_declarations_are_checked_for_duplicates() {
    assert!(
        error("struct P { x: int } struct P { y: int } fn f() -> [] int { return 0; }")
            .contains("declared twice")
    );
    assert!(
        error("struct P { x: int, x: bool } fn f() -> [] int { return 0; }")
            .contains("field `x` is declared twice")
    );
    assert!(
        error("struct int { x: int } fn f() -> [] int { return 0; }").contains("built-in type")
    );
}

#[test]
fn a_struct_may_mention_one_declared_later() {
    assert!(
        lower_src("struct A { b: B } struct B { x: int } fn f() -> [] int { return 0; }").is_ok()
    );
}

#[test]
fn structs_are_not_compared_with_equality() {
    assert!(
        error("struct P { x: int } fn f(a: P, b: P) -> [] bool { return a == b; }")
            .contains("cannot be compared")
    );
}

// ---- enums and match -------------------------------------------------

const SHAPE: &str = "enum Shape { Empty, Circle(int), Rect(int, int) } ";

#[test]
fn a_variant_becomes_an_index_and_a_payload() {
    let f = main_fn(&format!("{SHAPE}fn f() -> [] Shape {{ return Shape::Rect(2, 3); }}"));
    let Stmt::Return(Expr::Enum { variant, payload, .. }) = &f.body[0] else { panic!() };
    assert_eq!(*variant, 2);
    assert_eq!(payload, &vec![Expr::Int(2), Expr::Int(3)]);
}

#[test]
fn a_match_must_cover_every_variant() {
    let message = error(&format!(
        "{SHAPE}fn f(s: Shape) -> [] int {{ match s {{ Shape::Empty => {{ return 0; }} }} }}"
    ));
    assert!(message.contains("does not cover"), "{message}");
    assert!(message.contains("`Shape::Circle`"), "{message}");
    assert!(message.contains("`Shape::Rect`"), "{message}");
}

#[test]
fn a_wildcard_covers_the_rest() {
    assert!(
            lower_src(&format!(
                "{SHAPE}fn f(s: Shape) -> [] int {{ match s {{ Shape::Empty => {{ return 0; }} _ => {{ return 1; }} }} }}"
            ))
            .is_ok()
        );
}

#[test]
fn a_wildcard_that_covers_nothing_is_refused() {
    // Every variant is already matched, so the `_` can never run. Saying
    // so is worth more than silently allowing dead code.
    let message = error(&format!(
        "{SHAPE}fn f(s: Shape) -> [] int {{ match s {{              Shape::Empty => {{ return 0; }} Shape::Circle(r) => {{ return r; }}              Shape::Rect(w, h) => {{ return w * h; }} _ => {{ return 9; }} }} }}"
    ));
    assert!(message.contains("already matched"), "{message}");
}

#[test]
fn arms_after_a_wildcard_are_refused() {
    let message = error(&format!(
        "{SHAPE}fn f(s: Shape) -> [] int {{ match s {{ _ => {{ return 0; }} Shape::Empty => {{ return 1; }} }} }}"
    ));
    assert!(message.contains("unreachable"), "{message}");
}

#[test]
fn a_variant_may_not_be_matched_twice() {
    let message = error(&format!(
        "{SHAPE}fn f(s: Shape) -> [] int {{ match s {{              Shape::Empty => {{ return 0; }} Shape::Empty => {{ return 1; }} _ => {{ return 2; }} }} }}"
    ));
    assert!(message.contains("matched twice"), "{message}");
}

#[test]
fn payload_arity_is_checked_when_building_and_when_matching() {
    assert!(
        error(&format!("{SHAPE}fn f() -> [] Shape {{ return Shape::Rect(1); }}"))
            .contains("carries 2 values, but 1 was given")
    );
    let message = error(&format!(
        "{SHAPE}fn f(s: Shape) -> [] int {{ match s {{ Shape::Rect(w) => {{ return w; }} _ => {{ return 0; }} }} }}"
    ));
    assert!(message.contains("the pattern binds 1"), "{message}");
}

#[test]
fn a_binding_takes_the_payload_type() {
    // `r` is an `int`, so returning it from an `int` function is fine and
    // returning it from a `bool` one is not.
    assert!(
            lower_src(&format!(
                "{SHAPE}fn f(s: Shape) -> [] int {{ match s {{ Shape::Circle(r) => {{ return r; }} _ => {{ return 0; }} }} }}"
            ))
            .is_ok()
        );
    assert!(
            error(&format!(
                "{SHAPE}fn f(s: Shape) -> [] bool {{ match s {{ Shape::Circle(r) => {{ return r; }} _ => {{ return true; }} }} }}"
            ))
            .contains("expected `bool`, found `int`")
        );
}

#[test]
fn two_arms_may_bind_the_same_name_at_different_types() {
    assert!(
            lower_src(
                "enum E { A(int), B(bool) }                  fn f(e: E) -> [] int { match e { E::A(v) => { return v; } E::B(v) => { if v { return 1; } return 0; } } }"
            )
            .is_ok()
        );
}

#[test]
fn an_exhaustive_match_where_every_arm_returns_is_a_terminator() {
    // No trailing `return` needed: the match itself covers every path.
    assert!(
            lower_src(&format!(
                "{SHAPE}fn f(s: Shape) -> [] int {{ match s {{                  Shape::Empty => {{ return 0; }} Shape::Circle(r) => {{ return r; }}                  Shape::Rect(w, h) => {{ return w * h; }} }} }}"
            ))
            .is_ok()
        );
}

#[test]
fn only_enums_are_matched() {
    assert!(
        error("fn f() -> [] int { match 1 { _ => { return 0; } } }")
            .contains("`int` cannot be matched")
    );
    assert!(
        error("struct P { x: int } fn f(p: P) -> [] int { match p { _ => { return 0; } } }")
            .contains("is a struct, not an enum")
    );
}

#[test]
fn an_enum_that_contains_itself_is_refused() {
    assert!(
        error("enum List { Nil, Cons(int, List) } fn f() -> [] int { return 0; }")
            .contains("contains itself")
    );
}

#[test]
fn an_enum_needs_at_least_one_variant() {
    assert!(error("enum Void { } fn f() -> [] int { return 0; }").contains("has no variants"));
}

#[test]
fn structs_and_enums_are_not_interchangeable() {
    assert!(
        error("struct P { x: int } fn f() -> [] int { let p = P::x(1); return 0; }")
            .contains("is a struct, not an enum")
    );
    assert!(
        error("enum E { A } fn f() -> [] int { let e = E { x: 1 }; return 0; }")
            .contains("is an enum, not a struct")
    );
    assert!(
        error("enum E { A(int) } fn f(e: E) -> [] int { return e.x; }")
            .contains("read by matching on it")
    );
}

// ---- generics --------------------------------------------------------

fn names(src: &str) -> Vec<String> {
    let mut names: Vec<String> =
        lower_src(src).expect("should check").funcs.into_iter().map(|f| f.name).collect();
    names.sort();
    names
}

#[test]
fn a_generic_function_is_copied_once_per_instantiation() {
    let names = names(
        "fn id[T](x: T) -> [] T { return x; }              fn main() -> [] int { if id(true) { return id(1); } return id(2); }",
    );
    // One copy per type, not per call: `id(1)` and `id(2)` share theirs.
    assert_eq!(names, ["id$bool", "id$int", "main"]);
}

#[test]
fn a_generic_function_nobody_calls_is_emitted_nowhere() {
    let names = names("fn unused[T](x: T) -> [] T { return x; } fn main() -> [] int { return 0; }");
    assert_eq!(names, ["main"]);
}

#[test]
fn a_generic_body_is_checked_even_when_it_is_never_called() {
    // The point of checking rigidly: an error in a generic function does
    // not wait for someone to instantiate it.
    assert!(
        error("fn unused[T](x: T) -> [] int { return true; } fn main() -> [] int { return 0; }")
            .contains("expected `int`, found `bool`")
    );
}

#[test]
fn a_type_parameter_is_rigid_inside_the_body() {
    // `T` is not `int`, however every instantiation so far might be.
    let message =
        error("fn bad[T](x: T) -> [] T { return x + 1; } fn main() -> [] int { return 0; }");
    assert!(message.contains("expected `T`"), "{message}");
}

#[test]
fn type_arguments_are_inferred_from_the_arguments() {
    assert!(
        lower_src("fn id[T](x: T) -> [] T { return x; } fn main() -> [] int { return id(1); }")
            .is_ok()
    );
    assert!(
            error(
                // `[T: val]` because it drops `b`, which is only legal
                // for a copyable type (`docs/mode-polymorphism.md` §3.1);
                // without the bound this would be refused for *that*
                // rather than for the mismatch it is testing.
                "fn same[T: val](a: T, b: T) -> [] T { return a; }                  fn main() -> [] int { return same(1, true); }"
            )
            .contains("expected `int`, found `bool`")
        );
}

#[test]
fn a_type_argument_the_arguments_do_not_settle_is_refused() {
    let message = error(
        "enum Opt[T] { None, Some(T) }              fn make[T]() -> [] Opt[T] { return Opt::None; }              fn main() -> [] int { let x = make(); return 0; }",
    );
    assert!(message.contains("cannot tell what `T` is"), "{message}");
}

#[test]
fn a_generic_struct_substitutes_its_arguments_into_field_types() {
    assert!(
            lower_src(
                "struct Pair[A, B] { first: A, second: B }                  fn main() -> [] int { let p = Pair { first: 1, second: true };                  if p.second { return p.first; } return 0; }"
            )
            .is_ok()
        );
    assert!(
            error(
                "struct Pair[A, B] { first: A, second: B }                  fn main() -> [] int { let p = Pair { first: 1, second: true }; return p.second; }"
            )
            .contains("expected `int`, found `bool`")
        );
}

#[test]
fn a_generic_enum_substitutes_its_arguments_into_payloads() {
    assert!(
            lower_src(
                "enum Opt[T] { None, Some(T) }                  fn f(o: Opt[int]) -> [] int { match o { Opt::None => { return 0; } Opt::Some(v) => { return v; } } }"
            )
            .is_ok()
        );
    // The binding is an `int` here, so returning it as a `bool` is wrong.
    assert!(
            error(
                "enum Opt[T] { None, Some(T) }                  fn f(o: Opt[int]) -> [] bool { match o { Opt::None => { return true; } Opt::Some(v) => { return v; } } }"
            )
            .contains("expected `bool`, found `int`")
        );
}

#[test]
fn a_nullary_variant_takes_its_type_from_the_context() {
    // Nothing in `Opt::None` says what `T` is; the annotation does.
    assert!(
            lower_src(
                "enum Opt[T] { None, Some(T) }                  fn main() -> [] int { let x: Opt[int] = Opt::None; return 0; }"
            )
            .is_ok()
        );
    assert!(
        error("enum Opt[T] { None, Some(T) } fn main() -> [] int { let x = Opt::None; return 0; }")
            .contains("cannot tell what type")
    );
}

#[test]
fn type_argument_counts_are_checked() {
    assert!(
            error(
                "struct Pair[A, B] { first: A, second: B }                  fn f(p: Pair[int]) -> [] int { return p.first; }"
            )
            .contains("takes 2 type arguments, but 1 was given")
        );
    assert!(
        error("fn f(x: int[bool]) -> [] int { return 0; }").contains("takes no type arguments")
    );
    assert!(
        error("fn f[T](x: T[int]) -> [] int { return 0; }")
            .contains("type parameter `T` takes no type arguments")
    );
}

#[test]
fn type_parameter_names_are_checked() {
    assert!(error("fn f[T, T](x: T) -> [] T { return x; }").contains("declared twice"));
    assert!(error("fn f[int](x: int) -> [] int { return x; }").contains("built-in type"));
    assert!(
        error("struct S[A, A] { x: A } fn main() -> [] int { return 0; }")
            .contains("declared twice")
    );
}

#[test]
fn a_signature_is_checked_against_never_a_body() {
    // `g`'s body returns a bool, and its signature says int. The call in
    // `f` is checked against the signature, so the error is reported in
    // `g` -- a caller never learns anything from a callee's body.
    let message = error("fn g() -> [] int { return true; } fn f() -> [] int { return g(); }");
    assert!(message.contains("expected `int`, found `bool`"), "{message}");
}
