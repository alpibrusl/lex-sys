//! Recursive-descent parser with precedence climbing.
//!
//! The parser refuses on the first error: M0 wants a located refusal, not
//! recovery. Multi-error recovery is M1 work (#1).

use crate::ast::*;
use crate::lexer::{Token, TokenKind, tokenize};
use crate::span::{Diagnostic, Span};

pub fn parse(source: &str) -> Result<Ast, Diagnostic> {
    let tokens = tokenize(source)?;
    let mut p = Parser { source, tokens, pos: 0, ast: Ast::default(), no_struct_literal: false };
    p.unit()?;
    Ok(p.ast)
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    ast: Ast,
    /// True while parsing the condition of an `if` or `while`, where
    /// `Point { .. }` cannot be told from the body's opening brace. Rust has
    /// the same ambiguity and resolves it the same way: no struct literal
    /// here, and parentheses if you really meant one.
    no_struct_literal: bool,
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
            match self.peek().kind {
                TokenKind::Fn => self.fn_decl()?,
                TokenKind::Struct => self.struct_decl()?,
                TokenKind::Enum => self.enum_decl()?,
                other => {
                    return Err(self.err(format!(
                        "expected `fn`, `struct` or `enum`, found {}",
                        other.describe()
                    )));
                }
            };
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
            let ty = self.type_expr()?;
            params.push(Param { name, ty });
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RParen)?;

        // M0 has no unit type, so every function states a return type.
        self.expect(TokenKind::Arrow)?;
        let ret = self.type_expr()?;

        let (body, end) = self.block()?;
        Ok(self.ast.push_item(Item::Fn(FnDecl { name, params, ret, body }), start.to(end)))
    }

    fn struct_decl(&mut self) -> Result<ItemId, Diagnostic> {
        let start = self.expect(TokenKind::Struct)?.span;
        let name = self.ident()?;
        self.expect(TokenKind::LBrace)?;

        let mut fields = Vec::new();
        while self.peek().kind != TokenKind::RBrace {
            let name = self.ident()?;
            self.expect(TokenKind::Colon)?;
            let ty = self.type_expr()?;
            fields.push(FieldDecl { name, ty });
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        let end = self.expect(TokenKind::RBrace)?.span;
        Ok(self.ast.push_item(Item::Struct(StructDecl { name, fields }), start.to(end)))
    }

    fn enum_decl(&mut self) -> Result<ItemId, Diagnostic> {
        let start = self.expect(TokenKind::Enum)?.span;
        let name = self.ident()?;
        self.expect(TokenKind::LBrace)?;

        let mut variants = Vec::new();
        while self.peek().kind != TokenKind::RBrace {
            let name = self.ident()?;
            let mut payload = Vec::new();
            if self.eat(TokenKind::LParen) {
                while self.peek().kind != TokenKind::RParen {
                    payload.push(self.type_expr()?);
                    if !self.eat(TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(TokenKind::RParen)?;
            }
            variants.push(VariantDecl { name, payload });
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        let end = self.expect(TokenKind::RBrace)?.span;
        Ok(self.ast.push_item(Item::Enum(EnumDecl { name, variants }), start.to(end)))
    }

    fn ident(&mut self) -> Result<Symbol, Diagnostic> {
        let tok = self.expect(TokenKind::Ident)?;
        Ok(self.ast.symbols.intern(self.text(tok)))
    }

    /// A written type: a name, optionally applied to arguments.
    ///
    /// The parser does not know which names are types. `int`, `Pair[int, bool]`
    /// and `i32` all parse; only the last is an error, and saying so is the
    /// checker's job.
    fn type_expr(&mut self) -> Result<TypeId, Diagnostic> {
        let tok = self.peek();
        let name = self.ident()?;
        let mut args = Vec::new();
        let mut end = tok.span;
        if self.eat(TokenKind::LBracket) {
            while self.peek().kind != TokenKind::RBracket {
                args.push(self.type_expr()?);
                if !self.eat(TokenKind::Comma) {
                    break;
                }
            }
            end = self.expect(TokenKind::RBracket)?.span;
        }
        Ok(self.ast.push_type(TypeExpr { name, args }, tok.span.to(end)))
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
            TokenKind::Match => self.match_stmt(),
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
        let ty = if self.eat(TokenKind::Colon) { Some(self.type_expr()?) } else { None };
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

    /// An `if`/`while` condition: an expression that may not be a bare struct
    /// literal, because its brace would be taken for the body's.
    fn condition(&mut self) -> Result<ExprId, Diagnostic> {
        let outer = self.no_struct_literal;
        self.no_struct_literal = true;
        let cond = self.expr();
        self.no_struct_literal = outer;
        cond
    }

    fn if_stmt(&mut self) -> Result<StmtId, Diagnostic> {
        let kw = self.bump();
        let cond = self.condition()?;
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

    fn match_stmt(&mut self) -> Result<StmtId, Diagnostic> {
        let kw = self.bump();
        // The scrutinee sits where an `if` condition would, so it has the same
        // struct-literal ambiguity and the same answer.
        let scrutinee = self.condition()?;
        self.expect(TokenKind::LBrace)?;

        let mut arms = Vec::new();
        while self.peek().kind != TokenKind::RBrace {
            if self.peek().kind == TokenKind::Eof {
                return Err(self.err("expected `}`, found end of file"));
            }
            let pattern = self.pattern()?;
            self.expect(TokenKind::FatArrow)?;
            let (body, _) = self.block()?;
            arms.push(MatchArm { pattern, body });
            // Arms are brace-delimited, so a separating comma is optional.
            self.eat(TokenKind::Comma);
        }
        let end = self.expect(TokenKind::RBrace)?.span;
        Ok(self.ast.push_stmt(Stmt::Match { scrutinee, arms }, kw.span.to(end)))
    }

    fn pattern(&mut self) -> Result<Pattern, Diagnostic> {
        if self.eat(TokenKind::Underscore) {
            return Ok(Pattern::Wildcard);
        }
        let enum_name = self.ident()?;
        self.expect(TokenKind::ColonColon)?;
        let variant = self.ident()?;

        let mut bindings = Vec::new();
        if self.eat(TokenKind::LParen) {
            while self.peek().kind != TokenKind::RParen {
                if self.eat(TokenKind::Underscore) {
                    bindings.push(None);
                } else {
                    bindings.push(Some(self.ident()?));
                }
                if !self.eat(TokenKind::Comma) {
                    break;
                }
            }
            self.expect(TokenKind::RParen)?;
        }
        Ok(Pattern::Variant { enum_name, variant, bindings })
    }

    fn while_stmt(&mut self) -> Result<StmtId, Diagnostic> {
        let kw = self.bump();
        let cond = self.condition()?;
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
            &[(TokenKind::PipePipe, BinOp::Or)],
            &[(TokenKind::AmpAmp, BinOp::And)],
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
        if self.peek().kind == TokenKind::Bang {
            let bang = self.bump();
            let operand = self.unary()?;
            let span = bang.span.to(self.ast.expr_span(operand));
            return Ok(self.ast.push_expr(Expr::Unary { op: UnOp::Not, operand }, span));
        }
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
        self.postfix()
    }

    /// Field access binds tighter than any operator: `-p.x` negates the field,
    /// and `a.x + b.y` adds two fields.
    fn postfix(&mut self) -> Result<ExprId, Diagnostic> {
        let mut base = self.primary()?;
        while self.peek().kind == TokenKind::Dot {
            self.bump();
            let tok = self.peek();
            let name = self.ident()?;
            let span = self.ast.expr_span(base).to(tok.span);
            base = self.ast.push_expr(Expr::Field { base, name }, span);
        }
        Ok(base)
    }

    /// Parse `inner` with struct literals allowed again: inside brackets of any
    /// kind there is no brace to confuse with a block.
    fn bracketed<T>(
        &mut self,
        inner: impl FnOnce(&mut Self) -> Result<T, Diagnostic>,
    ) -> Result<T, Diagnostic> {
        let outer = self.no_struct_literal;
        self.no_struct_literal = false;
        let result = inner(self);
        self.no_struct_literal = outer;
        result
    }

    fn primary(&mut self) -> Result<ExprId, Diagnostic> {
        let tok = self.peek();
        match tok.kind {
            TokenKind::Int => {
                self.bump();
                let value = self.int_value(tok, false)?;
                Ok(self.ast.push_expr(Expr::Int(value), tok.span))
            }
            TokenKind::True | TokenKind::False => {
                self.bump();
                Ok(self.ast.push_expr(Expr::Bool(tok.kind == TokenKind::True), tok.span))
            }
            TokenKind::Ident => {
                let name = self.ident()?;
                match self.peek().kind {
                    TokenKind::ColonColon => {
                        self.bump();
                        let variant = self.ident()?;
                        let mut args = Vec::new();
                        let mut end = self.tokens[self.pos - 1].span;
                        if self.eat(TokenKind::LParen) {
                            args = self.bracketed(|p| {
                                let mut args = Vec::new();
                                while p.peek().kind != TokenKind::RParen {
                                    args.push(p.expr()?);
                                    if !p.eat(TokenKind::Comma) {
                                        break;
                                    }
                                }
                                Ok(args)
                            })?;
                            end = self.expect(TokenKind::RParen)?.span;
                        }
                        Ok(self.ast.push_expr(
                            Expr::Variant { enum_name: name, variant, args },
                            tok.span.to(end),
                        ))
                    }
                    TokenKind::LParen => {
                        self.bump();
                        let args = self.bracketed(|p| {
                            let mut args = Vec::new();
                            while p.peek().kind != TokenKind::RParen {
                                args.push(p.expr()?);
                                if !p.eat(TokenKind::Comma) {
                                    break;
                                }
                            }
                            Ok(args)
                        })?;
                        let end = self.expect(TokenKind::RParen)?.span;
                        Ok(self.ast.push_expr(Expr::Call { callee: name, args }, tok.span.to(end)))
                    }
                    TokenKind::LBrace if !self.no_struct_literal => {
                        self.bump();
                        let fields = self.bracketed(|p| {
                            let mut fields = Vec::new();
                            while p.peek().kind != TokenKind::RBrace {
                                let field = p.ident()?;
                                p.expect(TokenKind::Colon)?;
                                fields.push((field, p.expr()?));
                                if !p.eat(TokenKind::Comma) {
                                    break;
                                }
                            }
                            Ok(fields)
                        })?;
                        let end = self.expect(TokenKind::RBrace)?.span;
                        Ok(self.ast.push_expr(Expr::StructLit { name, fields }, tok.span.to(end)))
                    }
                    _ => Ok(self.ast.push_expr(Expr::Name(name), tok.span)),
                }
            }
            TokenKind::LParen => {
                // Grouping leaves no node behind: parentheses are formatting.
                self.bump();
                let inner = self.bracketed(|p| p.expr())?;
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

    /// Parse, and hand back the first function. Items other than functions
    /// are skipped, so a fixture may declare a struct alongside it.
    fn one_fn(src: &str) -> (Ast, FnDecl) {
        let ast = parse(src).expect("should parse");
        let decl = ast
            .items
            .iter()
            .find_map(|item| match item {
                Item::Fn(decl) => Some(decl.clone()),
                Item::Struct(_) | Item::Enum(_) => None,
            })
            .expect("a function");
        (ast, decl)
    }

    #[test]
    fn parses_a_function_with_params() {
        let (ast, decl) = one_fn("fn add(a: int, b: int) -> int { return a + b; }");
        assert_eq!(ast.name_of(decl.name), "add");
        assert_eq!(decl.params.len(), 2);
        assert_eq!(ast.name_of(ast.ty(decl.ret).name), "int");
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
    fn logical_operators_bind_loosest_of_all() {
        // `a < b && c < d` is `(a < b) && (c < d)`, and `||` is looser still.
        let (ast, decl) = one_fn("fn f() -> bool { return 1 < 2 && 3 < 4 || false; }");
        let Stmt::Return(e) = ast.stmt(decl.body.stmts[0]) else { panic!() };
        let Expr::Binary { op: BinOp::Or, lhs, .. } = ast.expr(*e) else { panic!() };
        let Expr::Binary { op: BinOp::And, lhs: inner, .. } = ast.expr(*lhs) else { panic!() };
        assert!(matches!(ast.expr(*inner), Expr::Binary { op: BinOp::Lt, .. }));
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
    fn an_unknown_type_name_parses_fine() {
        // The parser does not know which types exist. `i32` is a perfectly good
        // type *expression*; that it names nothing is for the checker to say,
        // which is also the only place that knows what the names mean.
        let (ast, decl) = one_fn("fn f() -> i32 { return 0; }");
        assert_eq!(ast.name_of(ast.ty(decl.ret).name), "i32");
        assert_eq!(ast.type_span(decl.ret), crate::span::Span::new(10, 13));
    }

    #[test]
    fn a_type_may_take_arguments() {
        let (ast, decl) = one_fn("fn f() -> Pair[int, bool] { return 0; }");
        let ret = ast.ty(decl.ret);
        assert_eq!(ast.name_of(ret.name), "Pair");
        let args: Vec<&str> = ret.args.iter().map(|&a| ast.name_of(ast.ty(a).name)).collect();
        assert_eq!(args, ["int", "bool"]);
    }

    #[test]
    fn booleans_are_literals() {
        let (ast, decl) = one_fn("fn f() -> bool { return !true; }");
        let Stmt::Return(e) = ast.stmt(decl.body.stmts[0]) else { panic!() };
        let Expr::Unary { op: UnOp::Not, operand } = ast.expr(*e) else { panic!() };
        assert_eq!(ast.expr(*operand), &Expr::Bool(true));
    }

    #[test]
    fn an_omitted_annotation_is_left_for_the_checker() {
        let (ast, decl) = one_fn("fn f() -> int { let x: int = 1; let y = 2; return x + y; }");
        let Stmt::Let { ty: Some(_), .. } = ast.stmt(decl.body.stmts[0]) else { panic!() };
        let Stmt::Let { ty: None, .. } = ast.stmt(decl.body.stmts[1]) else { panic!() };
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
    fn parses_a_struct_declaration() {
        let ast = parse("struct Point { x: int, y: bool }").unwrap();
        let Item::Struct(decl) = &ast.items[0] else { panic!() };
        assert_eq!(ast.name_of(decl.name), "Point");
        let fields: Vec<(&str, &str)> = decl
            .fields
            .iter()
            .map(|f| (ast.name_of(f.name), ast.name_of(ast.ty(f.ty).name)))
            .collect();
        assert_eq!(fields, [("x", "int"), ("y", "bool")]);
    }

    #[test]
    fn parses_a_struct_literal_and_field_access() {
        let (ast, decl) =
            one_fn("fn f() -> int { let p = Point { x: 1, y: 2 }; return p.x + p.y; }");
        let Stmt::Let { value, .. } = ast.stmt(decl.body.stmts[0]) else { panic!() };
        let Expr::StructLit { name, fields } = ast.expr(*value) else { panic!() };
        assert_eq!(ast.name_of(*name), "Point");
        assert_eq!(fields.len(), 2);

        let Stmt::Return(e) = ast.stmt(decl.body.stmts[1]) else { panic!() };
        let Expr::Binary { lhs, .. } = ast.expr(*e) else { panic!() };
        let Expr::Field { name, .. } = ast.expr(*lhs) else { panic!() };
        assert_eq!(ast.name_of(*name), "x");
    }

    #[test]
    fn field_access_binds_tighter_than_any_operator() {
        // `-p.x` negates the field, not the struct.
        let (ast, decl) = one_fn("fn f() -> int { return -p.x; }");
        let Stmt::Return(e) = ast.stmt(decl.body.stmts[0]) else { panic!() };
        let Expr::Unary { op: UnOp::Neg, operand } = ast.expr(*e) else { panic!() };
        assert!(matches!(ast.expr(*operand), Expr::Field { .. }));
    }

    #[test]
    fn a_condition_does_not_swallow_the_body_as_a_struct_literal() {
        // `if p { }` is a condition and a body, not a literal `p { }`.
        let (ast, decl) = one_fn("fn f() -> int { if p { return 1; } return 0; }");
        let Stmt::If { cond, .. } = ast.stmt(decl.body.stmts[0]) else { panic!() };
        assert!(matches!(ast.expr(*cond), Expr::Name(_)));

        // Parentheses are how you say you meant the literal.
        let (ast, decl) = one_fn("fn f() -> int { if (P { b: true }).b { return 1; } return 0; }");
        let Stmt::If { cond, .. } = ast.stmt(decl.body.stmts[0]) else { panic!() };
        assert!(matches!(ast.expr(*cond), Expr::Field { .. }));
    }

    #[test]
    fn a_struct_literal_is_allowed_again_inside_brackets() {
        let (ast, decl) = one_fn("fn f() -> int { if g(P { x: 1 }) { return 1; } return 0; }");
        let Stmt::If { cond, .. } = ast.stmt(decl.body.stmts[0]) else { panic!() };
        let Expr::Call { args, .. } = ast.expr(*cond) else { panic!() };
        assert!(matches!(ast.expr(args[0]), Expr::StructLit { .. }));
    }

    #[test]
    fn items_other_than_functions_are_refused() {
        let err = parse("let x = 1;").unwrap_err();
        assert!(err.message.contains("expected `fn`"), "{}", err.message);
    }
}
