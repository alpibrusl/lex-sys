//! Statements: a block, and everything that can start one -- `let`,
//! `return`, `defer`, `if`, `match`, `while`, `borrow`, `region` -- plus
//! the pattern a `let` or `match` arm can destructure.

use super::*;

impl<'a> Parser<'a> {
    // ---- statements ----------------------------------------------------

    /// Returns the block and the span of its closing brace.
    pub(crate) fn block(&mut self) -> Result<(Block, Span), Diagnostic> {
        self.expect(TokenKind::LBrace)?;
        let mut stmts = Vec::new();
        while self.peek().kind != TokenKind::RBrace {
            if self.peek().kind == TokenKind::Eof {
                return Err(self.err(Rule::TypeMismatch, "expected `}`, found end of file"));
            }
            stmts.push(self.stmt()?);
        }
        let end = self.expect(TokenKind::RBrace)?.span;
        Ok((Block { stmts }, end))
    }

    pub(crate) fn stmt(&mut self) -> Result<StmtId, Diagnostic> {
        match self.peek().kind {
            TokenKind::Let | TokenKind::Var => self.let_stmt(),
            TokenKind::Return => self.return_stmt(),
            TokenKind::Defer => self.defer_stmt(),
            TokenKind::If => self.if_stmt(),
            TokenKind::While => self.while_stmt(),
            TokenKind::Match => self.match_stmt(),
            TokenKind::Borrow => self.borrow_stmt(),
            TokenKind::Region => self.region_stmt(),
            // An expression statement, or an assignment to whatever that
            // expression turns out to name. M0 has no assignment expression,
            // so the `=` decides between them after the fact rather than by
            // looking ahead -- which is what lets the left side grow from a
            // name to `r.field` without the lookahead growing with it.
            _ => {
                let first = self.expr()?;
                if self.eat(TokenKind::Eq) {
                    let value = self.expr()?;
                    let end = self.expect(TokenKind::Semi)?.span;
                    let span = self.ast.expr_span(first).to(end);
                    return Ok(self.ast.push_stmt(Stmt::Assign { place: first, value }, span));
                }
                let end = self.expect(TokenKind::Semi)?.span;
                let span = self.ast.expr_span(first).to(end);
                Ok(self.ast.push_stmt(Stmt::Expr(first), span))
            }
        }
    }

    pub(crate) fn let_stmt(&mut self) -> Result<StmtId, Diagnostic> {
        let kw = self.bump();
        let mutable = kw.kind == TokenKind::Var;

        // `let (a, b) = t;` — taking a tuple apart (`docs/tuples.md` §3.2).
        // Decided by the parenthesis, which cannot otherwise follow `let`.
        // The names are the pattern's own, not a type's: a tuple has no
        // field names to inherit, which is the whole reason this pattern
        // may rename anything.
        if self.peek().kind == TokenKind::LParen {
            if mutable {
                return Err(self.err(
                    Rule::PatternShape,
                    "a destructuring binding takes a value apart once; write `let`, not `var`",
                ));
            }
            self.bump();
            let mut names = Vec::new();
            while self.peek().kind != TokenKind::RParen {
                // `let ((a, b), c) = t;`. This language has no nested
                // patterns anywhere (`docs/tuples.md` §4), and saying so is
                // worth more than "expected an identifier": the answer is
                // two statements, and the message should be the one that
                // says which two.
                if self.peek().kind == TokenKind::LParen {
                    return Err(self.err(Rule::PatternShape,
                        "a pattern does not nest; bind the inner tuple to a name here and take it apart in the next statement",
                    ));
                }
                names.push(self.ident()?);
                if !self.eat(TokenKind::Comma) {
                    break;
                }
            }
            self.expect(TokenKind::RParen)?;
            self.expect(TokenKind::Eq)?;
            let value = self.expr()?;
            let end = self.expect(TokenKind::Semi)?.span;
            return Ok(self
                .ast
                .push_stmt(Stmt::DestructureTuple { names, value }, kw.span.to(end)));
        }

        // `let io.Pair { a, b } = p;` — a struct reached through an
        // imported module (`docs/modules.md` §4). Two tokens of lookahead:
        // a `.` after a binding name can only be a qualifier here, since
        // the thing being bound is a name and never a field access.
        let first = self.ident()?;
        let (pattern_qualifier, name) = if self.peek().kind == TokenKind::Dot
            && self.peek_kind(1) == TokenKind::Ident
            && self.peek_kind(2) == TokenKind::LBrace
        {
            self.bump();
            (Some(first), self.ident()?)
        } else {
            (None, first)
        };

        // `let Point { x, y } = p;` — taking a value apart rather than naming
        // it. Decided by the brace, which cannot otherwise follow a binding.
        if self.peek().kind == TokenKind::LBrace {
            if mutable {
                return Err(self.err(
                    Rule::PatternShape,
                    "a destructuring binding takes a value apart once; write `let`, not `var`",
                ));
            }
            self.bump();
            let mut fields = Vec::new();
            while self.peek().kind != TokenKind::RBrace {
                fields.push(self.ident()?);
                if !self.eat(TokenKind::Comma) {
                    break;
                }
            }
            self.expect(TokenKind::RBrace)?;
            self.expect(TokenKind::Eq)?;
            let value = self.expr()?;
            let end = self.expect(TokenKind::Semi)?.span;
            return Ok(self.ast.push_stmt(
                Stmt::Destructure {
                    struct_name: name,
                    qualifier: pattern_qualifier,
                    fields,
                    value,
                },
                kw.span.to(end),
            ));
        }

        // The annotation is optional and can only be `int`; M1 gives it work to do.
        let ty = if self.eat(TokenKind::Colon) { Some(self.type_expr()?) } else { None };
        self.expect(TokenKind::Eq)?;
        let value = self.expr()?;
        let end = self.expect(TokenKind::Semi)?.span;
        Ok(self.ast.push_stmt(Stmt::Let { name, mutable, ty, value }, kw.span.to(end)))
    }

    pub(crate) fn return_stmt(&mut self) -> Result<StmtId, Diagnostic> {
        let kw = self.bump();
        let value = self.expr()?;
        let end = self.expect(TokenKind::Semi)?.span;
        Ok(self.ast.push_stmt(Stmt::Return(value), kw.span.to(end)))
    }

    /// `defer E;` (`docs/defer.md`).
    ///
    /// One expression, exactly like an expression statement -- because that
    /// is what it becomes, at every exit from this block instead of here.
    pub(crate) fn defer_stmt(&mut self) -> Result<StmtId, Diagnostic> {
        let kw = self.bump();
        let value = self.expr()?;
        let end = self.expect(TokenKind::Semi)?.span;
        Ok(self.ast.push_stmt(Stmt::Defer(value), kw.span.to(end)))
    }

    /// An `if`/`while` condition: an expression that may not be a bare struct
    /// literal, because its brace would be taken for the body's.
    pub(crate) fn condition(&mut self) -> Result<ExprId, Diagnostic> {
        let outer = self.no_struct_literal;
        self.no_struct_literal = true;
        let cond = self.expr();
        self.no_struct_literal = outer;
        cond
    }

    pub(crate) fn if_stmt(&mut self) -> Result<StmtId, Diagnostic> {
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

    pub(crate) fn match_stmt(&mut self) -> Result<StmtId, Diagnostic> {
        let kw = self.bump();
        // The scrutinee sits where an `if` condition would, so it has the same
        // struct-literal ambiguity and the same answer.
        let scrutinee = self.condition()?;
        self.expect(TokenKind::LBrace)?;

        let mut arms = Vec::new();
        while self.peek().kind != TokenKind::RBrace {
            if self.peek().kind == TokenKind::Eof {
                return Err(self.err(Rule::TypeMismatch, "expected `}`, found end of file"));
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

    pub(crate) fn pattern(&mut self) -> Result<Pattern, Diagnostic> {
        if self.eat(TokenKind::Underscore) {
            return Ok(Pattern::Wildcard);
        }
        // `m.Shape::Round` — the enum reached through an import
        // (`docs/modules.md` §4). Two tokens of lookahead is enough here,
        // unlike in an expression: a pattern position has no field access
        // to tell it apart from, so `a.B` can only be a qualified name.
        let qualifier = if self.peek_kind(1) == TokenKind::Dot {
            let qualifier = self.ident()?;
            self.expect(TokenKind::Dot)?;
            Some(qualifier)
        } else {
            None
        };
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
        Ok(Pattern::Variant { enum_name, qualifier, variant, bindings })
    }

    pub(crate) fn while_stmt(&mut self) -> Result<StmtId, Diagnostic> {
        let kw = self.bump();
        let cond = self.condition()?;
        let (body, end) = self.block()?;
        Ok(self.ast.push_stmt(Stmt::While { cond, body }, kw.span.to(end)))
    }

    /// `borrow x as &r in { .. }` / `borrow mut x as &!r in { .. }`.
    ///
    /// The `&` and the `!` are written at the binder for the same reason they
    /// are written in a type: a reader should not have to look anywhere else
    /// to know whether this freezes `x` or locks it. The two must agree, so
    /// `borrow mut x as &r` is refused here rather than silently picking one.
    pub(crate) fn borrow_stmt(&mut self) -> Result<StmtId, Diagnostic> {
        let kw = self.bump();
        let unique = self.eat(TokenKind::Mut);
        let value = self.ident()?;
        self.expect(TokenKind::As)?;
        self.expect(TokenKind::Amp)?;
        let bang = self.eat(TokenKind::Bang);
        if bang != unique {
            return Err(self.err(
                Rule::PatternShape,
                if unique {
                    "`borrow mut` binds a unique reference; write `as &!r`"
                } else {
                    "`as &!r` binds a unique reference; write `borrow mut`"
                },
            ));
        }
        let region = self.ident()?;
        self.expect(TokenKind::In)?;
        let (body, end) = self.block()?;
        Ok(self.ast.push_stmt(Stmt::Borrow { value, unique, region, body }, kw.span.to(end)))
    }

    /// `region a { .. }` (§6).
    ///
    /// The region is written bare. `&` is the reference constructor, and
    /// there is nothing here for it to construct -- what follows `region` is
    /// a region name and can be nothing else.
    pub(crate) fn region_stmt(&mut self) -> Result<StmtId, Diagnostic> {
        let kw = self.bump();
        let region = self.ident()?;
        let (body, end) = self.block()?;
        Ok(self.ast.push_stmt(Stmt::Region { region, body }, kw.span.to(end)))
    }
}
