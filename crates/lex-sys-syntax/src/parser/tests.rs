use super::*;

/// Parse, and hand back the first function. Items other than functions
/// are skipped, so a fixture may declare a struct alongside it.
fn one_fn(src: &str) -> (Ast, FnDecl) {
    let ast = parse(src).expect("should parse");
    let decl = ast
        .items
        .iter()
        .find_map(|item| match item {
            Item::Fn(decl) => Some(decl.clone()),
            Item::Struct(_) | Item::Enum(_) | Item::Extern(_) | Item::Static(_) => None,
        })
        .expect("a function");
    (ast, decl)
}

#[test]
fn parses_a_function_with_params() {
    let (ast, decl) = one_fn("fn add(a: int, b: int) -> [] int { return a + b; }");
    assert_eq!(ast.name_of(decl.name), "add");
    assert_eq!(decl.params.len(), 2);
    assert_eq!(ast.name_of(ast.ty(decl.ret).head().expect("a named type")), "int");
    assert_eq!(decl.body.stmts.len(), 1);
}

#[test]
fn multiplication_binds_tighter_than_addition() {
    let (ast, decl) = one_fn("fn f() -> [] int { return 1 + 2 * 3; }");
    let Stmt::Return(e) = ast.stmt(decl.body.stmts[0]) else { panic!() };
    let Expr::Binary { op, rhs, .. } = ast.expr(*e) else { panic!() };
    assert_eq!(*op, BinOp::Add);
    assert!(matches!(ast.expr(*rhs), Expr::Binary { op: BinOp::Mul, .. }));
}

#[test]
fn logical_operators_bind_loosest_of_all() {
    // `a < b && c < d` is `(a < b) && (c < d)`, and `||` is looser still.
    let (ast, decl) = one_fn("fn f() -> [] bool { return 1 < 2 && 3 < 4 || false; }");
    let Stmt::Return(e) = ast.stmt(decl.body.stmts[0]) else { panic!() };
    let Expr::Binary { op: BinOp::Or, lhs, .. } = ast.expr(*e) else { panic!() };
    let Expr::Binary { op: BinOp::And, lhs: inner, .. } = ast.expr(*lhs) else { panic!() };
    assert!(matches!(ast.expr(*inner), Expr::Binary { op: BinOp::Lt, .. }));
}

#[test]
fn comparison_binds_looser_than_arithmetic() {
    let (ast, decl) = one_fn("fn f() -> [] int { return 1 + 2 < 4; }");
    let Stmt::Return(e) = ast.stmt(decl.body.stmts[0]) else { panic!() };
    let Expr::Binary { op, lhs, .. } = ast.expr(*e) else { panic!() };
    assert_eq!(*op, BinOp::Lt);
    assert!(matches!(ast.expr(*lhs), Expr::Binary { op: BinOp::Add, .. }));
}

#[test]
fn subtraction_is_left_associative() {
    let (ast, decl) = one_fn("fn f() -> [] int { return 10 - 3 - 2; }");
    let Stmt::Return(e) = ast.stmt(decl.body.stmts[0]) else { panic!() };
    let Expr::Binary { op: BinOp::Sub, lhs, .. } = ast.expr(*e) else { panic!() };
    assert!(matches!(ast.expr(*lhs), Expr::Binary { op: BinOp::Sub, .. }));
}

#[test]
fn parentheses_leave_no_node_behind() {
    let with = parse("fn f() -> [] int { return (1 + 2); }").unwrap();
    let without = parse("fn f() -> [] int { return 1 + 2; }").unwrap();
    // Canonical shape: grouping is formatting, so the arenas match exactly.
    assert_eq!(with.exprs, without.exprs);
    assert_eq!(with.stmts, without.stmts);
}

#[test]
fn layout_does_not_change_the_arenas() {
    let a = parse("fn f() -> [] int { return 1+2; }").unwrap();
    let b = parse("fn f()  -> [] int {\n    // sum\n    return 1 + 2;\n}\n").unwrap();
    assert_eq!(a.exprs, b.exprs);
    assert_eq!(a.stmts, b.stmts);
    assert_eq!(a.items, b.items);
}

#[test]
fn else_if_becomes_a_nested_block() {
    let (ast, decl) =
        one_fn("fn f() -> [] int { if 1 { return 1; } else if 2 { return 2; } return 0; }");
    let Stmt::If { else_block: Some(block), .. } = ast.stmt(decl.body.stmts[0]) else { panic!() };
    assert_eq!(block.stmts.len(), 1);
    assert!(matches!(ast.stmt(block.stmts[0]), Stmt::If { .. }));
}

#[test]
fn assignment_is_a_statement_not_an_expression() {
    let (ast, decl) = one_fn("fn f() -> [] int { var x = 1; x = 2; return x; }");
    assert!(matches!(ast.stmt(decl.body.stmts[1]), Stmt::Assign { .. }));
    assert!(parse("fn f() -> [] int { var x = 1; return (x = 2); }").is_err());
}

#[test]
fn negative_literals_are_single_nodes() {
    let (ast, decl) = one_fn("fn f() -> [] int { return -9223372036854775808; }");
    let Stmt::Return(e) = ast.stmt(decl.body.stmts[0]) else { panic!() };
    assert_eq!(ast.expr(*e), &Expr::Int(i64::MIN));
}

#[test]
fn an_oversized_literal_is_refused() {
    let err = parse("fn f() -> [] int { return 9223372036854775808; }").unwrap_err();
    assert!(err.message.contains("does not fit"), "{}", err.message);
    assert!(parse("fn f() -> [] int { return -9223372036854775809; }").is_err());
}

#[test]
fn underscores_separate_digits() {
    let (ast, decl) = one_fn("fn f() -> [] int { return 1_000_000; }");
    let Stmt::Return(e) = ast.stmt(decl.body.stmts[0]) else { panic!() };
    assert_eq!(ast.expr(*e), &Expr::Int(1_000_000));
}

#[test]
fn an_unknown_type_name_parses_fine() {
    // The parser does not know which types exist. `i32` is a perfectly good
    // type *expression*; that it names nothing is for the checker to say,
    // which is also the only place that knows what the names mean.
    let (ast, decl) = one_fn("fn f() -> [] i32 { return 0; }");
    assert_eq!(ast.name_of(ast.ty(decl.ret).head().expect("a named type")), "i32");
    assert_eq!(ast.type_span(decl.ret), crate::span::Span::new(13, 16));
}

#[test]
fn a_type_may_take_arguments() {
    let (ast, decl) = one_fn("fn f() -> [] Pair[int, bool] { return 0; }");
    let ret = ast.ty(decl.ret);
    assert_eq!(ast.name_of(ret.head().expect("a named type")), "Pair");
    let args: Vec<&str> =
        ret.args().iter().map(|&a| ast.name_of(ast.ty(a).head().unwrap())).collect();
    assert_eq!(args, ["int", "bool"]);
}

#[test]
fn booleans_are_literals() {
    let (ast, decl) = one_fn("fn f() -> [] bool { return !true; }");
    let Stmt::Return(e) = ast.stmt(decl.body.stmts[0]) else { panic!() };
    let Expr::Unary { op: UnOp::Not, operand } = ast.expr(*e) else { panic!() };
    assert_eq!(ast.expr(*operand), &Expr::Bool(true));
}

#[test]
fn an_omitted_annotation_is_left_for_the_checker() {
    let (ast, decl) = one_fn("fn f() -> [] int { let x: int = 1; let y = 2; return x + y; }");
    let Stmt::Let { ty: Some(_), .. } = ast.stmt(decl.body.stmts[0]) else { panic!() };
    let Stmt::Let { ty: None, .. } = ast.stmt(decl.body.stmts[1]) else { panic!() };
}

#[test]
fn a_missing_return_type_is_refused() {
    assert!(parse("fn f() { return 0; }").is_err());
}

#[test]
fn an_unclosed_block_is_refused() {
    let err = parse("fn f() -> [] int { return 0;").unwrap_err();
    assert!(err.message.contains("end of file"), "{}", err.message);
}

#[test]
fn a_trailing_comma_in_a_call_is_allowed() {
    let (ast, decl) = one_fn("fn f() -> [] int { return g(1, 2,); }");
    let Stmt::Return(e) = ast.stmt(decl.body.stmts[0]) else { panic!() };
    let Expr::Call { args, .. } = ast.expr(*e) else { panic!() };
    assert_eq!(args.len(), 2);
}

#[test]
fn parses_a_struct_declaration() {
    let ast = parse("struct Point { x: int, y: bool }").unwrap();
    let Item::Struct(decl) = &ast.items[0] else { panic!() };
    assert_eq!(ast.name_of(decl.name), "Point");
    let fields: Vec<(&str, &str)> = decl
        .fields
        .iter()
        .map(|f| (ast.name_of(f.name), ast.name_of(ast.ty(f.ty).head().unwrap())))
        .collect();
    assert_eq!(fields, [("x", "int"), ("y", "bool")]);
}

#[test]
fn parses_a_struct_literal_and_field_access() {
    let (ast, decl) =
        one_fn("fn f() -> [] int { let p = Point { x: 1, y: 2 }; return p.x + p.y; }");
    let Stmt::Let { value, .. } = ast.stmt(decl.body.stmts[0]) else { panic!() };
    let Expr::StructLit { name, fields, .. } = ast.expr(*value) else { panic!() };
    assert_eq!(ast.name_of(*name), "Point");
    assert_eq!(fields.len(), 2);

    let Stmt::Return(e) = ast.stmt(decl.body.stmts[1]) else { panic!() };
    let Expr::Binary { lhs, .. } = ast.expr(*e) else { panic!() };
    let Expr::Field { name, .. } = ast.expr(*lhs) else { panic!() };
    assert_eq!(ast.name_of(*name), "x");
}

/// `docs/tuples.md` §2.1: the comma is what decides, and grouping keeps
/// the meaning it had before tuples existed.
///
/// This is the whole argument for "two components or more" in one
/// test: `(e)` cannot become a one-tuple without breaking every
/// parenthesis already written, so the rule is chosen to leave it
/// alone.
#[test]
fn a_parenthesis_is_grouping_until_a_comma_makes_it_a_tuple() {
    let (ast, decl) = one_fn("fn f() -> [] int { return (1 + 2) * 3; }");
    let Stmt::Return(e) = ast.stmt(decl.body.stmts[0]) else { panic!() };
    // Grouping leaves no node behind, so this is a product whose left
    // operand is a sum -- there is no one-tuple anywhere in it.
    let Expr::Binary { op: BinOp::Mul, lhs, .. } = ast.expr(*e) else { panic!() };
    assert!(matches!(ast.expr(*lhs), Expr::Binary { op: BinOp::Add, .. }));

    let (ast, decl) = one_fn("fn f() -> [] int { return (1, 2); }");
    let Stmt::Return(e) = ast.stmt(decl.body.stmts[0]) else { panic!() };
    let Expr::Tuple(parts) = ast.expr(*e) else { panic!("a comma makes a tuple") };
    assert_eq!(parts.len(), 2);
}

/// §3.1: `t.0` needs nothing from the lexer.
///
/// There are no float literals in this language, so `0` after a dot is
/// an integer token and nothing else -- which is why `t.0.1` parses as
/// two postfix accesses rather than as a number that has to be taken
/// apart again.
#[test]
fn a_positional_field_chains() {
    // `t.0.1` is two components, not one and the float `0.1`. The
    // lexer settles it with one bit of context -- a number straight
    // after a dot is an index -- which floating-point literals made
    // necessary and `docs/floating-point.md` §1 records.
    let (ast, decl) = one_fn("fn f() -> [] int { return t.0.1; }");
    let Stmt::Return(e) = ast.stmt(decl.body.stmts[0]) else { panic!() };
    let Expr::TupleField { base, index } = ast.expr(*e) else { panic!() };
    assert_eq!(*index, 1);
    let Expr::TupleField { index, .. } = ast.expr(*base) else { panic!() };
    assert_eq!(*index, 0);
}

#[test]
fn field_access_binds_tighter_than_any_operator() {
    // `-p.x` negates the field, not the struct.
    let (ast, decl) = one_fn("fn f() -> [] int { return -p.x; }");
    let Stmt::Return(e) = ast.stmt(decl.body.stmts[0]) else { panic!() };
    let Expr::Unary { op: UnOp::Neg, operand } = ast.expr(*e) else { panic!() };
    assert!(matches!(ast.expr(*operand), Expr::Field { .. }));
}

#[test]
fn a_condition_does_not_swallow_the_body_as_a_struct_literal() {
    // `if p { }` is a condition and a body, not a literal `p { }`.
    let (ast, decl) = one_fn("fn f() -> [] int { if p { return 1; } return 0; }");
    let Stmt::If { cond, .. } = ast.stmt(decl.body.stmts[0]) else { panic!() };
    assert!(matches!(ast.expr(*cond), Expr::Name(_)));

    // Parentheses are how you say you meant the literal.
    let (ast, decl) = one_fn("fn f() -> [] int { if (P { b: true }).b { return 1; } return 0; }");
    let Stmt::If { cond, .. } = ast.stmt(decl.body.stmts[0]) else { panic!() };
    assert!(matches!(ast.expr(*cond), Expr::Field { .. }));
}

#[test]
fn a_struct_literal_is_allowed_again_inside_brackets() {
    let (ast, decl) = one_fn("fn f() -> [] int { if g(P { x: 1 }) { return 1; } return 0; }");
    let Stmt::If { cond, .. } = ast.stmt(decl.body.stmts[0]) else { panic!() };
    let Expr::Call { args, .. } = ast.expr(*cond) else { panic!() };
    assert!(matches!(ast.expr(args[0]), Expr::StructLit { .. }));
}

#[test]
fn items_other_than_functions_are_refused() {
    let err = parse("let x = 1;").unwrap_err();
    assert!(err.message.contains("expected `fn`"), "{}", err.message);
}

// ---- modes and destructuring (`docs/linearity-and-effects.md` §3, §4.1)

#[test]
fn a_declaration_may_carry_a_mode() {
    let ast = parse("res struct File { fd: int } val struct P { x: int } enum E { A }")
        .expect("should parse");
    let Item::Struct(file) = &ast.items[0] else { panic!() };
    let Item::Struct(point) = &ast.items[1] else { panic!() };
    let Item::Enum(e) = &ast.items[2] else { panic!() };
    assert_eq!(file.mode, Some(Mode::Res));
    assert_eq!(point.mode, Some(Mode::Val));
    assert_eq!(e.mode, None);
}

#[test]
fn a_mode_belongs_to_a_type_declaration() {
    let err = parse("res fn f() -> [] int { return 0; }").unwrap_err();
    assert!(err.message.contains("expected `struct` or `enum`"), "{}", err.message);
}

#[test]
fn the_mode_keyword_is_part_of_the_declaration_span() {
    // The span decides where a diagnostic about the declaration points,
    // and `val struct P { f: File }` is refused *because of* the `val`.
    let ast = parse("res struct File { fd: int }").expect("should parse");
    let span = ast.item_span(ItemId(0));
    assert_eq!(span.start, 0);
}

#[test]
fn a_let_may_destructure() {
    let (ast, decl) = one_fn("fn f(p: P) -> [] int { let P { x, y } = p; return x + y; }");
    let Stmt::Destructure { struct_name, fields, value, .. } = ast.stmt(decl.body.stmts[0]) else {
        panic!("expected a destructuring `let`")
    };
    assert_eq!(ast.name_of(*struct_name), "P");
    let names: Vec<&str> = fields.iter().map(|f| ast.name_of(*f)).collect();
    assert_eq!(names, ["x", "y"]);
    assert!(matches!(ast.expr(*value), Expr::Name(_)));
}

#[test]
fn a_destructuring_let_is_not_a_var() {
    let err = parse("fn f(p: P) -> [] int { var P { x } = p; return x; }").unwrap_err();
    assert!(err.message.contains("write `let`, not `var`"), "{}", err.message);
}

#[test]
fn an_ordinary_let_is_still_an_ordinary_let() {
    let (ast, decl) = one_fn("fn f() -> [] int { let x = P { a: 1 }; return 0; }");
    assert!(matches!(ast.stmt(decl.body.stmts[0]), Stmt::Let { .. }));
}

// ---- borrowing (`docs/linearity-and-effects.md` §5) -----------------

#[test]
fn a_reference_type_names_its_region_first() {
    let (ast, decl) = one_fn("fn f(s: &r Bytes) -> [] int { return 0; }");
    let TypeExpr::Ref { unique, region, inner } = ast.ty(decl.params[0].ty) else {
        panic!("expected a reference type")
    };
    assert!(!unique);
    assert_eq!(ast.name_of(*region), "r");
    assert_eq!(ast.name_of(ast.ty(*inner).head().unwrap()), "Bytes");
}

#[test]
fn a_unique_reference_wears_a_bang() {
    let (ast, decl) = one_fn("fn f(s: &!r Bytes) -> [] int { return 0; }");
    let TypeExpr::Ref { unique, .. } = ast.ty(decl.params[0].ty) else { panic!() };
    assert!(unique);
}

#[test]
fn references_nest() {
    let (ast, decl) = one_fn("fn f(s: &a &b int) -> [] int { return 0; }");
    let TypeExpr::Ref { region, inner, .. } = ast.ty(decl.params[0].ty) else { panic!() };
    assert_eq!(ast.name_of(*region), "a");
    let TypeExpr::Ref { region, .. } = ast.ty(*inner) else { panic!("expected a reference") };
    assert_eq!(ast.name_of(*region), "b");
}

#[test]
fn a_region_parameter_wears_its_ampersand_at_the_binder() {
    let (ast, decl) = one_fn("fn f[T, &r](x: T, s: &r T) -> [] int { return 0; }");
    let types: Vec<&str> = decl.generics.iter().map(|g| ast.name_of(*g)).collect();
    let regions: Vec<&str> = decl.regions.iter().map(|g| ast.name_of(*g)).collect();
    assert_eq!(types, ["T"]);
    assert_eq!(regions, ["r"]);
}

#[test]
fn a_region_parameter_nobody_uses_is_still_a_region() {
    // The point of marking the binder: this declaration is unambiguous
    // even though no parameter mentions `r`.
    let (ast, decl) = one_fn("fn f[&r]() -> [] int { return 0; }");
    assert_eq!(decl.regions.len(), 1);
    assert_eq!(ast.name_of(decl.regions[0]), "r");
    assert!(decl.generics.is_empty());
}

#[test]
fn a_where_clause_declares_an_outlives_pair() {
    let (ast, decl) = one_fn(
        "fn f[&dst, &src where src <= dst](d: &dst int, s: &src int) -> [] int { return 0; }",
    );
    let pairs: Vec<(&str, &str)> =
        decl.outlives.iter().map(|(a, b)| (ast.name_of(*a), ast.name_of(*b))).collect();
    assert_eq!(pairs, [("src", "dst")]);
}

#[test]
fn a_type_declaration_takes_no_region_parameters() {
    let err = parse("struct S[&r] { x: int }").unwrap_err();
    assert!(err.message.contains("no region parameters"), "{}", err.message);
}

#[test]
fn a_borrow_statement_binds_a_region_and_a_reference() {
    let (ast, decl) = one_fn("fn f(x: int) -> [] int { borrow x as &r in { return 0; } }");
    let Stmt::Borrow { value, unique, region, body } = ast.stmt(decl.body.stmts[0]) else {
        panic!("expected a borrow")
    };
    assert_eq!(ast.name_of(*value), "x");
    assert!(!unique);
    assert_eq!(ast.name_of(*region), "r");
    assert_eq!(body.stmts.len(), 1);
}

#[test]
fn borrow_mut_binds_a_unique_reference() {
    let (ast, decl) = one_fn("fn f(x: int) -> [] int { borrow mut x as &!r in { return 0; } }");
    let Stmt::Borrow { unique, .. } = ast.stmt(decl.body.stmts[0]) else { panic!() };
    assert!(unique);
}

#[test]
fn the_two_halves_of_a_borrow_must_agree() {
    // Writing `mut` in one place and not the other is a typo, not a
    // shorthand, so neither spelling is quietly preferred.
    let err = parse("fn f(x: int) -> [] int { borrow mut x as &r in { return 0; } }").unwrap_err();
    assert!(err.message.contains("write `as &!r`"), "{}", err.message);
    let err = parse("fn f(x: int) -> [] int { borrow x as &!r in { return 0; } }").unwrap_err();
    assert!(err.message.contains("write `borrow mut`"), "{}", err.message);
}

#[test]
fn an_assignment_target_is_an_expression() {
    // Decided by the `=` after the fact rather than by lookahead, which
    // is what lets the left side grow without the parser growing with it.
    let (ast, decl) = one_fn("fn f(p: P) -> [] int { p.x = 1; return 0; }");
    let Stmt::Assign { place, .. } = ast.stmt(decl.body.stmts[0]) else {
        panic!("expected an assignment")
    };
    assert!(matches!(ast.expr(*place), Expr::Field { .. }));

    let (ast, decl) = one_fn("fn f() -> [] int { var x = 0; x = 1; return x; }");
    let Stmt::Assign { place, .. } = ast.stmt(decl.body.stmts[1]) else { panic!() };
    assert!(matches!(ast.expr(*place), Expr::Name(_)));
}

#[test]
fn a_comparison_is_still_an_expression_statement() {
    // `==` is one token, so it never reaches the assignment branch.
    let (ast, decl) = one_fn("fn f(a: int) -> [] int { a == 1; return 0; }");
    assert!(matches!(ast.stmt(decl.body.stmts[0]), Stmt::Expr(_)));
}

// ---- effect rows (`docs/linearity-and-effects.md` §7) ---------------

#[test]
fn a_signature_carries_its_effect_row() {
    let (ast, decl) = one_fn("fn f() -> [io, fs] int { return 0; }");
    let labels: Vec<&str> = decl.effects.iter().map(|e| ast.name_of(e.name)).collect();
    assert_eq!(labels, ["io", "fs"], "the parser keeps what was written");
    assert!(decl.effects.iter().all(|e| e.argument.is_none()));
}

#[test]
fn a_region_block_names_its_region_bare() {
    // §6: `&` is the reference constructor, and there is nothing here
    // for it to construct.
    let (ast, decl) = one_fn("fn f() -> [] int { region a { let x = 1; } return 0; }");
    let Stmt::Region { region, body } = ast.stmt(decl.body.stmts[0]) else {
        panic!("a region statement");
    };
    assert_eq!(ast.name_of(*region), "a");
    assert_eq!(body.stmts.len(), 1);
    assert!(parse("fn f() -> [] int { region &a { } return 0; }").is_err());
}

#[test]
fn alloc_names_its_arena_in_brackets() {
    let (ast, decl) = one_fn("fn f() -> [] int { region a { let n = alloc[a](1); } return 0; }");
    let Stmt::Region { body, .. } = ast.stmt(decl.body.stmts[0]) else {
        panic!("a region statement");
    };
    let Stmt::Let { value, .. } = ast.stmt(body.stmts[0]) else { panic!("a binding") };
    let Expr::Alloc { region, .. } = ast.expr(*value) else { panic!("an allocation") };
    assert_eq!(ast.name_of(*region), "a");
}

#[test]
fn alloc_without_an_arena_is_not_a_call() {
    // The arena is written, never inferred: allocating somewhere the
    // author did not name is the ambient behaviour §6 refuses.
    let err =
        parse("fn f() -> [] int { let n = alloc(1); return 0; }").expect_err("should be refused");
    assert!(err.message.contains('['), "{}", err.message);
}

#[test]
fn a_label_may_carry_a_literal() {
    // §7.4: `ffi` and `ffi("libc")` are different labels, and the parser
    // keeps the difference rather than dropping the argument.
    let (ast, decl) = one_fn("fn f() -> [ffi(\"libc\"), io] int { return 0; }");
    let written: Vec<(String, Option<String>)> =
        decl.effects.iter().map(|e| (ast.name_of(e.name).to_owned(), e.argument.clone())).collect();
    assert_eq!(written, vec![("ffi".to_owned(), Some("libc".to_owned())), ("io".to_owned(), None)]);
}

#[test]
fn a_foreign_declaration_is_a_signature_with_no_body() {
    // §8.4. It is region-polymorphic like any other function, because
    // the capability that authorises it is borrowed.
    let ast = parse("extern fn labs[&f](ffi: &f Ffi(\"libc\"), n: int) -> [ffi(\"libc\")] int;")
        .expect("should parse");
    let decl = ast
        .items
        .iter()
        .find_map(|item| match item {
            Item::Extern(decl) => Some(decl.clone()),
            _ => None,
        })
        .expect("a foreign declaration");
    assert_eq!(ast.name_of(decl.name), "labs");
    assert_eq!(decl.symbol, "labs", "the linker binds the name it was given");
    assert_eq!(decl.params.len(), 2);
    assert_eq!(decl.regions.len(), 1);
}

#[test]
fn a_foreign_declaration_takes_no_type_parameters() {
    // There is nothing in C to instantiate one at.
    let err = parse("extern fn f[T](x: T) -> [] int;").expect_err("should be refused");
    assert!(err.message.contains("no type parameters"), "{}", err.message);
}

#[test]
fn an_empty_row_is_how_a_signature_says_pure() {
    let (_, decl) = one_fn("fn f() -> [] int { return 0; }");
    assert!(decl.effects.is_empty());
}

#[test]
fn a_row_is_required() {
    // §7.2: an absent row would be an inferred one.
    let err = parse("fn f() -> int { return 0; }").unwrap_err();
    assert!(err.message.contains("expected `[`"), "{}", err.message);
}

#[test]
fn a_row_does_not_collide_with_a_generic_return_type() {
    // `-> [] Opt[int]` parses as an empty row and a generic type; no type
    // starts with `[`, so there is nothing to disambiguate.
    let (ast, decl) = one_fn("fn f() -> [] Opt[int] { return g(); }");
    assert!(decl.effects.is_empty());
    assert_eq!(ast.name_of(ast.ty(decl.ret).head().unwrap()), "Opt");
}

#[test]
fn a_lone_ampersand_is_not_a_conjunction() {
    let (ast, decl) = one_fn("fn f(a: bool, b: bool) -> [] bool { return a && b; }");
    let Stmt::Return(value) = ast.stmt(decl.body.stmts[0]) else { panic!() };
    assert!(matches!(ast.expr(*value), Expr::Binary { op: BinOp::And, .. }));
}

/// The declaration a `res` aggregate needs (`docs/collections.md` §3):
/// the vector owns an allocation, and its elements are copyable.
#[test]
fn a_res_declaration_may_bound_its_parameters() {
    let ast = parse("res struct Vec[T: val] { held: Box[[T]], used: int }").expect("parses");
    let Item::Struct(decl) = &ast.items[0] else { panic!() };
    assert_eq!(decl.mode, Some(Mode::Res));
    assert_eq!(decl.bounds, vec![Some(Mode::Val)]);
}

/// And an undeclared one may too: its mode is *computed* from its
/// members, so a bound restricts what may instantiate it rather than
/// repeating something already said.
#[test]
fn an_undeclared_aggregate_may_bound_its_parameters() {
    let ast = parse("enum Pair[A: val, B] { One(A), Two(B) }").expect("parses");
    let Item::Enum(decl) = &ast.items[0] else { panic!() };
    assert_eq!(decl.mode, None);
    assert_eq!(decl.bounds, vec![Some(Mode::Val), None]);
}

/// A `val` one may not: saying `val` is the bound (§3).
#[test]
fn a_val_declaration_may_not_restate_its_bound() {
    let err = parse("val struct Wrap[T: val] { held: T }").unwrap_err();
    assert!(err.message.contains("already bounds its parameters"), "{}", err.message);
}

/// And there is no `res` bound on a type any more than on a function.
#[test]
fn a_type_takes_no_res_bound() {
    let err = parse("res struct Holder[T: res] { held: T }").unwrap_err();
    assert!(err.message.contains("no `res` bound"), "{}", err.message);
}

/// `docs/collections.md` §5: a `match` reaches an enum through the
/// same qualifier every other reference uses. Without it an imported
/// enum is a type a program can hold and never take apart.
#[test]
fn a_pattern_may_name_an_enum_through_a_qualifier() {
    let (ast, decl) =
        one_fn("fn f(s: m.Shape) -> [] int { match s { m.Shape::Flat => { return 0; } } }");
    let Stmt::Match { arms, .. } = ast.stmt(decl.body.stmts[0]) else { panic!() };
    let Pattern::Variant { enum_name, qualifier, variant, .. } = &arms[0].pattern else { panic!() };
    assert_eq!(ast.name_of(qualifier.expect("a qualifier")), "m");
    assert_eq!(ast.name_of(*enum_name), "Shape");
    assert_eq!(ast.name_of(*variant), "Flat");
}

/// A file with no `edition N;` marker is edition 1, forever
/// (`docs/editions.md` §6.1) — every fixture written before editions
/// existed is one of these.
#[test]
fn an_absent_edition_marker_defaults_to_one() {
    let (ast, decl) = one_fn("fn f() -> [] int { return 1; }");
    let item = ast.items.iter().position(|i| matches!(i, Item::Fn(d) if d.name == decl.name));
    assert_eq!(ast.edition_of(ItemId(item.unwrap() as u32)), 1);
}

/// `edition 1;` is a no-op: it names the language as it is today, and
/// items after it are edition 1 exactly as they would have been without
/// it.
#[test]
fn edition_one_is_accepted_as_a_no_op() {
    let (ast, decl) = one_fn("edition 1;\nfn f() -> [] int { return 1; }");
    let item = ast.items.iter().position(|i| matches!(i, Item::Fn(d) if d.name == decl.name));
    assert_eq!(ast.edition_of(ItemId(item.unwrap() as u32)), 1);
}

/// There is nothing later than edition 1 to opt into yet
/// (`docs/editions.md` §6.1), so any other number is refused rather
/// than silently accepted.
#[test]
fn an_unknown_edition_is_refused() {
    let err = parse("edition 2;\nfn f() -> [] int { return 1; }").unwrap_err();
    assert_eq!(err.rule, Rule::UnknownEdition);
    assert!(err.message.contains("unknown edition 2"), "{}", err.message);
}

/// The marker comes before even `module` (§6.1) — checked once, ahead
/// of the loop that parses items, so it cannot appear anywhere else in
/// the file.
#[test]
fn the_edition_marker_must_come_before_the_module_declaration() {
    let err = parse("module a;\nedition 1;\nfn f() -> [] int { return 1; }").unwrap_err();
    assert_eq!(err.rule, Rule::TypeMismatch);
}

/// And it may appear at most once — a second one is just an
/// unrecognized item at that point, the same refusal any other stray
/// identifier gets.
#[test]
fn the_edition_marker_may_appear_at_most_once() {
    let err = parse("edition 1;\nedition 1;\nfn f() -> [] int { return 1; }").unwrap_err();
    assert_eq!(err.rule, Rule::TypeMismatch);
}
