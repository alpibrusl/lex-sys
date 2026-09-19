//! Recursive-descent parser with precedence climbing.
//!
//! The parser refuses on the first error: M0 wants a located refusal, not
//! recovery. Multi-error recovery is M1 work (#1).

use crate::ast::*;
use crate::lexer::{Token, TokenKind, tokenize};
use crate::span::{Diagnostic, Span};

pub fn parse(source: &str) -> Result<Ast, Diagnostic> {
    let tokens = tokenize(source)?;
    let mut p = Parser { source, tokens, pos: 0, ast: Ast::default() };
    p.unit()?;
    Ok(p.ast)
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    ast: Ast,
}

impl<'a> Parser<'a> {
    // ---- token plumbing ------------------------------------------------

    fn peek(&self) -> Token {
        self.tokens[self.pos]
    }

    fn peek_at(&self, n: usize) -> Token {
        self.tokens[(self.pos + n).min(self.tokens.len() - 1)]
    }

    fn text(&self, tok: Token) -> &'a str {
        &self.source[tok.span.start as usize..tok.span.end as usize]
    }

    fn bump(&mut self) -> Token {
        let tok = self.peek();
        if tok.kind != TokenKind::Eof {
            self.pos += 1;
        }
        tok
    }

    fn eat(&mut self, kind: TokenKind) -> bool {
        if self.peek().kind == kind {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: TokenKind) -> Result<Token, Diagnostic> {
        let tok = self.peek();
        if tok.kind == kind {
            Ok(self.bump())
        } else {
            Err(self.err(format!("expected {}, found {}", kind.describe(), tok.kind.describe())))
        }
    }

    fn err(&self, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(message, self.peek().span)
    }

    // ---- items ---------------------------------------------------------

    fn unit(&mut self) -> Result<(), Diagnostic> {
        while self.peek().kind != TokenKind::Eof {
            if self.peek().kind != TokenKind::Fn {
                return Err(self.err(format!(
                    "expected `fn`, found {} (M0 has no items other than functions)",
                    self.peek().kind.describe()
                )));
            }
            self.fn_decl()?;
        }
        Ok(())
    }

    fn fn_decl(&mut self) -> Result<ItemId, Diagnostic> {
        let start = self.expect(TokenKind::Fn)?.span;
        let name = self.ident()?;

        self.expect(TokenKind::LParen)?;
        let mut params = Vec::new();
        while self.peek().kind != TokenKind::RParen {
            let name = self.ident()?;
            self.expect(TokenKind::Colon)?;
            let ty = self.type_ref()?;
            params.push(Param { name, ty });
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RParen)?;

        // M0 has no unit type, so every function states a return type.
        self.expect(TokenKind::Arrow)?;
        let ret = self.type_ref()?;

        let (body, end) = self.block()?;
        Ok(self.ast.push_item(Item::Fn(FnDecl { name, params, ret, body }), start.to(end)))
    }

    fn ident(&mut self) -> Result<Symbol, Diagnostic> {
        let tok = self.expect(TokenKind::Ident)?;
        Ok(self.ast.symbols.intern(self.text(tok)))
    }

    fn type_ref(&mut self) -> Result<TypeRef, Diagnostic> {
        let tok = self.expect(TokenKind::Ident)?;
        match self.text(tok) {
            "int" => Ok(TypeRef::Int),
            other => Err(Diagnostic::new(
                format!("unknown type `{other}` (M0 has one type: `int`)"),
                tok.span,
            )),
        }
    }

    // ---- statements ----------------------------------------------------

    /// Returns the block and the span of its closing brace.
    fn block(&mut self) -> Result<(Block, Span), Diagnostic> {
        self.expect(TokenKind::LBrace)?;
        let mut stmts = Vec::new();
        while self.peek().kind != TokenKind::RBrace {
            if self.peek().kind == TokenKind::Eof {
                return Err(self.err("expected `}`, found end of file"));
            }
            stmts.push(self.stmt()?);
        }
        let end = self.expect(TokenKind::RBrace)?.span;
        Ok((Block { stmts }, end))
    }

    fn stmt(&mut self) -> Result<StmtId, Diagnostic> {
        match self.peek().kind {
            TokenKind::Let | TokenKind::Var => self.let_stmt(),
            TokenKind::Return => self.return_stmt(),
            TokenKind::If => self.if_stmt(),
            TokenKind::While => self.while_stmt(),
            // `x = e;` — an assignment, not an expression: M0 has no
            // assignment expressions, so this is decided by lookahead.
            TokenKind::Ident if self.peek_at(1).kind == TokenKind::Eq => self.assign_stmt(),
            _ => {
                let value = self.expr()?;
                let end = self.expect(TokenKind::Semi)?.span;
                let span = self.ast.expr_span(value).to(end);
                Ok(self.ast.push_stmt(Stmt::Expr(value), span))
            }
        }
    }

    fn let_stmt(&mut self) -> Result<StmtId, Diagnostic> {
        let kw = self.bump();
        let mutable = kw.kind == TokenKind::Var;
        let name = self.ident()?;
        // The annotation is optional and can only be `int`; M1 gives it work to do.
        let ty = if self.eat(TokenKind::Colon) { self.type_ref()? } else { TypeRef::Int };
        self.expect(TokenKind::Eq)?;
        let value = self.expr()?;
        let end = self.expect(TokenKind::Semi)?.span;
        Ok(self.ast.push_stmt(Stmt::Let { name, mutable, ty, value }, kw.span.to(end)))
    }

    fn assign_stmt(&mut self) -> Result<StmtId, Diagnostic> {
        let name_tok = self.peek();
        let name = self.ident()?;
        self.expect(TokenKind::Eq)?;
        let value = self.expr()?;
        let end = self.expect(TokenKind::Semi)?.span;
        Ok(self.ast.push_stmt(Stmt::Assign { name, value }, name_tok.span.to(end)))
    }

    fn return_stmt(&mut self) -> Result<StmtId, Diagnostic> {
        let kw = self.bump();
        let value = self.expr()?;
        let end = self.expect(TokenKind::Semi)?.span;
        Ok(self.ast.push_stmt(Stmt::Return(value), kw.span.to(end)))
    }

    fn if_stmt(&mut self) -> Result<StmtId, Diagnostic> {
        let kw = self.bump();
        let cond = self.expr()?;
        let (then_block, mut end) = self.block()?;
        let mut else_block = None;
        if self.eat(TokenKind::Else) {
            if self.peek().kind == TokenKind::If {
                // `else if` is sugar for an else block holding one `if`.
                let nested = self.if_stmt()?;
                end = self.ast.stmt_span(nested);
                else_block = Some(Block { stmts: vec![nested] });
            } else {
                let (block, block_end) = self.block()?;
                end = block_end;
                else_block = Some(block);
            }
        }
        Ok(self.ast.push_stmt(Stmt::If { cond, then_block, else_block }, kw.span.to(end)))
    }

    fn while_stmt(&mut self) -> Result<StmtId, Diagnostic> {
        let kw = self.bump();
        let cond = self.expr()?;
        let (body, end) = self.block()?;
        Ok(self.ast.push_stmt(Stmt::While { cond, body }, kw.span.to(end)))
    }

    // ---- expressions ---------------------------------------------------

    fn expr(&mut self) -> Result<ExprId, Diagnostic> {
        self.equality()
    }

    fn equality(&mut self) -> Result<ExprId, Diagnostic> {
        self.binary_level(0)
    }

    /// Left-associative binary levels, lowest precedence first.
    fn binary_level(&mut self, level: usize) -> Result<ExprId, Diagnostic> {
        const LEVELS: &[&[(TokenKind, BinOp)]] = &[
            &[(TokenKind::EqEq, BinOp::Eq), (TokenKind::BangEq, BinOp::Ne)],
            &[
                (TokenKind::Lt, BinOp::Lt),
                (TokenKind::LtEq, BinOp::Le),
                (TokenKind::Gt, BinOp::Gt),
                (TokenKind::GtEq, BinOp::Ge),
            ],
            &[(TokenKind::Plus, BinOp::Add), (TokenKind::Minus, BinOp::Sub)],
            &[
                (TokenKind::Star, BinOp::Mul),
                (TokenKind::Slash, BinOp::Div),
                (TokenKind::Percent, BinOp::Rem),
            ],
        ];

        if level == LEVELS.len() {
            return self.unary();
        }

        let mut lhs = self.binary_level(level + 1)?;
        loop {
            let Some(&(_, op)) = LEVELS[level].iter().find(|(k, _)| *k == self.peek().kind) else {
                return Ok(lhs);
            };
            self.bump();
            let rhs = self.binary_level(level + 1)?;
            let span = self.ast.expr_span(lhs).to(self.ast.expr_span(rhs));
            lhs = self.ast.push_expr(Expr::Binary { op, lhs, rhs }, span);
        }
    }

    fn unary(&mut self) -> Result<ExprId, Diagnostic> {
        if self.peek().kind == TokenKind::Minus {
            let minus = self.bump();
            // `-9223372036854775808` is one literal, not a negated one: the
            // magnitude does not fit `int` on its own.
            if self.peek().kind == TokenKind::Int {
                let tok = self.bump();
                let value = self.int_value(tok, true)?;
                return Ok(self.ast.push_expr(Expr::Int(value), minus.span.to(tok.span)));
            }
            let operand = self.unary()?;
            let span = minus.span.to(self.ast.expr_span(operand));
            return Ok(self.ast.push_expr(Expr::Unary { op: UnOp::Neg, operand }, span));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<ExprId, Diagnostic> {
        let tok = self.peek();
        match tok.kind {
            TokenKind::Int => {
                self.bump();
                let value = self.int_value(tok, false)?;
                Ok(self.ast.push_expr(Expr::Int(value), tok.span))
            }
            TokenKind::Ident => {
                let name = self.ident()?;
                if self.peek().kind != TokenKind::LParen {
                    return Ok(self.ast.push_expr(Expr::Name(name), tok.span));
                }
                self.bump();
                let mut args = Vec::new();
                while self.peek().kind != TokenKind::RParen {
                    args.push(self.expr()?);
                    if !self.eat(TokenKind::Comma) {
                        break;
                    }
                }
                let end = self.expect(TokenKind::RParen)?.span;
                Ok(self.ast.push_expr(Expr::Call { callee: name, args }, tok.span.to(end)))
            }
            TokenKind::LParen => {
                // Grouping leaves no node behind: parentheses are formatting.
                self.bump();
                let inner = self.expr()?;
                self.expect(TokenKind::RParen)?;
                Ok(inner)
            }
            other => Err(self.err(format!("expected an expression, found {}", other.describe()))),
        }
    }

    fn int_value(&self, tok: Token, negated: bool) -> Result<i64, Diagnostic> {
        let digits: String = self.text(tok).chars().filter(|c| *c != '_').collect();
        let magnitude: u64 = digits.parse().map_err(|_| {
            Diagnostic::new("integer literal does not fit in `int` (64-bit signed)", tok.span)
        })?;
        let limit = if negated { 1u64 << 63 } else { i64::MAX as u64 };
        if magnitude > limit {
            return Err(Diagnostic::new(
                "integer literal does not fit in `int` (64-bit signed)",
                tok.span,
            ));
        }
        Ok(if negated { (magnitude as i64).wrapping_neg() } else { magnitude as i64 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_fn(src: &str) -> (Ast, FnDecl) {
        let ast = parse(src).expect("should parse");
        let Item::Fn(decl) = ast.items[0].clone();
        (ast, decl)
    }

    #[test]
    fn parses_a_function_with_params() {
        let (ast, decl) = one_fn("fn add(a: int, b: int) -> int { return a + b; }");
        assert_eq!(ast.name_of(decl.name), "add");
        assert_eq!(decl.params.len(), 2);
        assert_eq!(decl.ret, TypeRef::Int);
        assert_eq!(decl.body.stmts.len(), 1);
    }

    #[test]
    fn multiplication_binds_tighter_than_addition() {
        let (ast, decl) = one_fn("fn f() -> int { return 1 + 2 * 3; }");
        let Stmt::Return(e) = ast.stmt(decl.body.stmts[0]) else { panic!() };
        let Expr::Binary { op, rhs, .. } = ast.expr(*e) else { panic!() };
        assert_eq!(*op, BinOp::Add);
        assert!(matches!(ast.expr(*rhs), Expr::Binary { op: BinOp::Mul, .. }));
    }

    #[test]
    fn comparison_binds_looser_than_arithmetic() {
        let (ast, decl) = one_fn("fn f() -> int { return 1 + 2 < 4; }");
        let Stmt::Return(e) = ast.stmt(decl.body.stmts[0]) else { panic!() };
        let Expr::Binary { op, lhs, .. } = ast.expr(*e) else { panic!() };
        assert_eq!(*op, BinOp::Lt);
        assert!(matches!(ast.expr(*lhs), Expr::Binary { op: BinOp::Add, .. }));
    }

    #[test]
    fn subtraction_is_left_associative() {
        let (ast, decl) = one_fn("fn f() -> int { return 10 - 3 - 2; }");
        let Stmt::Return(e) = ast.stmt(decl.body.stmts[0]) else { panic!() };
        let Expr::Binary { op: BinOp::Sub, lhs, .. } = ast.expr(*e) else { panic!() };
        assert!(matches!(ast.expr(*lhs), Expr::Binary { op: BinOp::Sub, .. }));
    }

    #[test]
    fn parentheses_leave_no_node_behind() {
        let with = parse("fn f() -> int { return (1 + 2); }").unwrap();
        let without = parse("fn f() -> int { return 1 + 2; }").unwrap();
        // Canonical shape: grouping is formatting, so the arenas match exactly.
        assert_eq!(with.exprs, without.exprs);
        assert_eq!(with.stmts, without.stmts);
    }

    #[test]
    fn layout_does_not_change_the_arenas() {
        let a = parse("fn f() -> int { return 1+2; }").unwrap();
        let b = parse("fn f()  ->  int {\n    // sum\n    return 1 + 2;\n}\n").unwrap();
        assert_eq!(a.exprs, b.exprs);
        assert_eq!(a.stmts, b.stmts);
        assert_eq!(a.items, b.items);
    }

    #[test]
    fn else_if_becomes_a_nested_block() {
        let (ast, decl) =
            one_fn("fn f() -> int { if 1 { return 1; } else if 2 { return 2; } return 0; }");
        let Stmt::If { else_block: Some(block), .. } = ast.stmt(decl.body.stmts[0]) else {
            panic!()
        };
        assert_eq!(block.stmts.len(), 1);
        assert!(matches!(ast.stmt(block.stmts[0]), Stmt::If { .. }));
    }

    #[test]
    fn assignment_is_a_statement_not_an_expression() {
        let (ast, decl) = one_fn("fn f() -> int { var x = 1; x = 2; return x; }");
        assert!(matches!(ast.stmt(decl.body.stmts[1]), Stmt::Assign { .. }));
        assert!(parse("fn f() -> int { var x = 1; return (x = 2); }").is_err());
    }

    #[test]
    fn negative_literals_are_single_nodes() {
        let (ast, decl) = one_fn("fn f() -> int { return -9223372036854775808; }");
        let Stmt::Return(e) = ast.stmt(decl.body.stmts[0]) else { panic!() };
        assert_eq!(ast.expr(*e), &Expr::Int(i64::MIN));
    }

    #[test]
    fn an_oversized_literal_is_refused() {
        let err = parse("fn f() -> int { return 9223372036854775808; }").unwrap_err();
        assert!(err.message.contains("does not fit"), "{}", err.message);
        assert!(parse("fn f() -> int { return -9223372036854775809; }").is_err());
    }

    #[test]
    fn underscores_separate_digits() {
        let (ast, decl) = one_fn("fn f() -> int { return 1_000_000; }");
        let Stmt::Return(e) = ast.stmt(decl.body.stmts[0]) else { panic!() };
        assert_eq!(ast.expr(*e), &Expr::Int(1_000_000));
    }

    #[test]
    fn unknown_types_are_refused_with_a_span() {
        let err = parse("fn f() -> i32 { return 0; }").unwrap_err();
        assert!(err.message.contains("unknown type `i32`"), "{}", err.message);
        assert_eq!(err.span, crate::span::Span::new(10, 13));
    }

    #[test]
    fn a_missing_return_type_is_refused() {
        assert!(parse("fn f() { return 0; }").is_err());
    }

    #[test]
    fn an_unclosed_block_is_refused() {
        let err = parse("fn f() -> int { return 0;").unwrap_err();
        assert!(err.message.contains("end of file"), "{}", err.message);
    }

    #[test]
    fn a_trailing_comma_in_a_call_is_allowed() {
        let (ast, decl) = one_fn("fn f() -> int { return g(1, 2,); }");
        let Stmt::Return(e) = ast.stmt(decl.body.stmts[0]) else { panic!() };
        let Expr::Call { args, .. } = ast.expr(*e) else { panic!() };
        assert_eq!(args.len(), 2);
    }

    #[test]
    fn items_other_than_functions_are_refused() {
        let err = parse("let x = 1;").unwrap_err();
        assert!(err.message.contains("expected `fn`"), "{}", err.message);
    }
}
