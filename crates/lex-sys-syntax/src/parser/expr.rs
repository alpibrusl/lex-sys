//! Expressions: precedence climbing from `expr` down through every
//! binding level to `primary`, and the two literal parsers underneath it.

use super::*;

impl<'a> Parser<'a> {
    // ---- expressions ---------------------------------------------------

    pub(crate) fn expr(&mut self) -> Result<ExprId, Diagnostic> {
        self.equality()
    }

    pub(crate) fn equality(&mut self) -> Result<ExprId, Diagnostic> {
        self.binary_level(0)
    }

    /// Left-associative binary levels, lowest precedence first.
    pub(crate) fn binary_level(&mut self, level: usize) -> Result<ExprId, Diagnostic> {
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
            // The bit operators bind *tighter* than comparison, which is
            // Rust's ordering rather than C's: `flags & mask == 0` means
            // `(flags & mask) == 0` here, and in C it does not
            // (`docs/bitwise.md` §5).
            &[(TokenKind::Pipe, BinOp::BitOr)],
            &[(TokenKind::Caret, BinOp::BitXor)],
            &[(TokenKind::Amp, BinOp::BitAnd)],
            &[(TokenKind::LtLt, BinOp::Shl), (TokenKind::GtGt, BinOp::Shr)],
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

    pub(crate) fn unary(&mut self) -> Result<ExprId, Diagnostic> {
        if self.peek().kind == TokenKind::Bang {
            let bang = self.bump();
            let operand = self.unary()?;
            let span = bang.span.to(self.ast.expr_span(operand));
            return Ok(self.ast.push_expr(Expr::Unary { op: UnOp::Not, operand }, span));
        }
        if self.peek().kind == TokenKind::Star {
            let star = self.bump();
            let operand = self.unary()?;
            let span = star.span.to(self.ast.expr_span(operand));
            return Ok(self.ast.push_expr(Expr::Unary { op: UnOp::Deref, operand }, span));
        }
        if self.peek().kind == TokenKind::Tilde {
            let tilde = self.bump();
            let operand = self.unary()?;
            let span = tilde.span.to(self.ast.expr_span(operand));
            return Ok(self.ast.push_expr(Expr::Unary { op: UnOp::BitNot, operand }, span));
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
            // `-1.5` is one literal for the same reason `-x` is not: the
            // sign belongs to the number, and `-0.0` is a value a
            // negation of `0.0` would also produce but a *literal* should
            // be able to say directly (`docs/floating-point.md` §1).
            if self.peek().kind == TokenKind::Float {
                let tok = self.bump();
                let bits = self.float_value(tok, true)?;
                return Ok(self.ast.push_expr(Expr::Float(bits), minus.span.to(tok.span)));
            }
            let operand = self.unary()?;
            let span = minus.span.to(self.ast.expr_span(operand));
            return Ok(self.ast.push_expr(Expr::Unary { op: UnOp::Neg, operand }, span));
        }
        self.postfix()
    }

    /// Field access binds tighter than any operator: `-p.x` negates the field,
    /// and `a.x + b.y` adds two fields.
    pub(crate) fn postfix(&mut self) -> Result<ExprId, Diagnostic> {
        let mut base = self.primary()?;
        loop {
            match self.peek().kind {
                TokenKind::Dot => {
                    self.bump();
                    let tok = self.peek();
                    // `t.0` — a component by position (`docs/tuples.md`
                    // §3.1). There are no float literals in this language,
                    // so `0` after a dot is an integer token and nothing
                    // else, and `t.0.1` lexes as three dots' worth of
                    // postfix with no help from the lexer.
                    if tok.kind == TokenKind::Int {
                        self.bump();
                        let value = self.int_value(tok, false)?;
                        let Ok(index) = u32::try_from(value) else {
                            return Err(Diagnostic::new(
                                Rule::LiteralForm,
                                "a tuple component is named by its position, counting from 0",
                                tok.span,
                            ));
                        };
                        let span = self.ast.expr_span(base).to(tok.span);
                        base = self.ast.push_expr(Expr::TupleField { base, index }, span);
                        continue;
                    }
                    let name = self.ident()?;
                    let span = self.ast.expr_span(base).to(tok.span);
                    base = self.ast.push_expr(Expr::Field { base, name }, span);
                }
                // `s[i]`. Chains like `.field` does, so `rows[i].len` and
                // `grid[i][j]` need no special case.
                TokenKind::LBracket => {
                    self.bump();
                    // `s[i]` or `s[a..b]` — one bracket, told apart by the
                    // `..` after the first expression (`docs/slicing.md` §1).
                    let (index, range) = self.bracketed(|p| {
                        let first = p.expr()?;
                        if p.eat(TokenKind::DotDot) {
                            let second = p.expr()?;
                            return Ok((first, Some(second)));
                        }
                        Ok((first, None))
                    })?;
                    let end = self.expect(TokenKind::RBracket)?.span;
                    let span = self.ast.expr_span(base).to(end);
                    base = match range {
                        Some(last) => {
                            self.ast.push_expr(Expr::Slice { base, start: index, end: last }, span)
                        }
                        None => self.ast.push_expr(Expr::Index { base, index }, span),
                    };
                }
                _ => return Ok(base),
            }
        }
    }

    /// Parse `inner` with struct literals allowed again: inside brackets of any
    /// kind there is no brace to confuse with a block.
    pub(crate) fn bracketed<T>(
        &mut self,
        inner: impl FnOnce(&mut Self) -> Result<T, Diagnostic>,
    ) -> Result<T, Diagnostic> {
        let outer = self.no_struct_literal;
        self.no_struct_literal = false;
        let result = inner(self);
        self.no_struct_literal = outer;
        result
    }

    pub(crate) fn primary(&mut self) -> Result<ExprId, Diagnostic> {
        let tok = self.peek();
        match tok.kind {
            TokenKind::Int => {
                self.bump();
                let value = self.int_value(tok, false)?;
                Ok(self.ast.push_expr(Expr::Int(value), tok.span))
            }
            TokenKind::Float => {
                self.bump();
                let bits = self.float_value(tok, false)?;
                Ok(self.ast.push_expr(Expr::Float(bits), tok.span))
            }
            TokenKind::Str => {
                let text = self.string_literal()?;
                Ok(self.ast.push_expr(Expr::Str(text), tok.span))
            }
            TokenKind::True | TokenKind::False => {
                self.bump();
                Ok(self.ast.push_expr(Expr::Bool(tok.kind == TokenKind::True), tok.span))
            }
            TokenKind::Ident if self.text(tok) == "alloc_slice" => {
                // `alloc_slice[a](count, fill)`. Reserved like `alloc`, and
                // for the same reason: the brackets name a *region*, which
                // no other call site does, and postfix brackets on anything
                // else are an index.
                self.bump();
                self.expect(TokenKind::LBracket)?;
                let region = self.ident()?;
                self.expect(TokenKind::RBracket)?;
                self.expect(TokenKind::LParen)?;
                let (count, fill) = self.bracketed(|p| {
                    let count = p.expr()?;
                    p.expect(TokenKind::Comma)?;
                    Ok((count, p.expr()?))
                })?;
                let end = self.expect(TokenKind::RParen)?.span;
                Ok(self.ast.push_expr(Expr::AllocSlice { region, count, fill }, tok.span.to(end)))
            }
            TokenKind::Ident if self.text(tok) == "alloc" => {
                // `alloc[a](v)`: the one place brackets name a region at a
                // call site, so it is parsed as itself rather than as a call
                // that happens to be spelled oddly. The arena is written,
                // never inferred -- allocating somewhere the author did not
                // name is exactly the ambient behaviour §6 exists to refuse.
                self.bump();
                self.expect(TokenKind::LBracket)?;
                let region = self.ident()?;
                self.expect(TokenKind::RBracket)?;
                self.expect(TokenKind::LParen)?;
                let value = self.bracketed(|p| p.expr())?;
                let end = self.expect(TokenKind::RParen)?.span;
                Ok(self.ast.push_expr(Expr::Alloc { region, value }, tok.span.to(end)))
            }
            TokenKind::Ident => {
                // `io.print_nat(x)`, `io.Shape::Round`, `io.Buffer { .. }`
                // — a name reached through an imported module
                // (`docs/modules.md` §4).
                //
                // Three tokens of lookahead, because `p.x` is field access
                // and looks the same for two of them. What separates them
                // is the *third*: a qualified name is followed by `(`,
                // `::` or `{`, and a field never is -- this language has
                // no methods, so `p.x(..)` is not a thing that could mean
                // something else.
                let qualifier = if self.peek_kind(1) == TokenKind::Dot
                    && self.peek_kind(2) == TokenKind::Ident
                    && matches!(self.peek_kind(3), TokenKind::LParen | TokenKind::ColonColon)
                    || self.peek_kind(1) == TokenKind::Dot
                        && self.peek_kind(2) == TokenKind::Ident
                        && self.peek_kind(3) == TokenKind::LBrace
                        && !self.no_struct_literal
                {
                    let qualifier = self.ident()?;
                    self.expect(TokenKind::Dot)?;
                    Some(qualifier)
                } else {
                    None
                };
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
                            Expr::Variant { enum_name: name, qualifier, variant, args },
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
                        Ok(self.ast.push_expr(
                            Expr::Call { callee: name, qualifier, args },
                            tok.span.to(end),
                        ))
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
                        Ok(self.ast.push_expr(
                            Expr::StructLit { name, qualifier, fields },
                            tok.span.to(end),
                        ))
                    }
                    _ => {
                        if qualifier.is_some() {
                            return Err(self.err(Rule::UnknownName,
                                "a qualified name reaches a function, a type or a variant in that module, not a value",
                            ));
                        }
                        Ok(self.ast.push_expr(Expr::Name(name), tok.span))
                    }
                }
            }
            TokenKind::LParen => {
                // Grouping leaves no node behind: parentheses are formatting.
                // A comma after the first expression makes it a tuple
                // instead (`docs/tuples.md` §2.1) -- one token of lookahead.
                // `(e)` is already spoken for, which is why there is no
                // one-tuple; `(e,)` still parses, as a one-part tuple the
                // checker refuses, so that the refusal names the rule.
                self.bump();
                let first = self.bracketed(|p| p.expr())?;
                if !self.eat(TokenKind::Comma) {
                    self.expect(TokenKind::RParen)?;
                    return Ok(first);
                }
                let mut parts = vec![first];
                while self.peek().kind != TokenKind::RParen {
                    parts.push(self.bracketed(|p| p.expr())?);
                    if !self.eat(TokenKind::Comma) {
                        break;
                    }
                }
                let end = self.expect(TokenKind::RParen)?.span;
                Ok(self.ast.push_expr(Expr::Tuple(parts), tok.span.to(end)))
            }
            other => Err(self.err(
                Rule::TypeMismatch,
                format!("expected an expression, found {}", other.describe()),
            )),
        }
    }

    /// A floating-point literal's bits.
    ///
    /// Rust's `f64::from_str` is correctly rounded, so `0.1` is the
    /// nearest binary64 to one tenth and not something near it. A literal
    /// too large to represent is refused where it is written rather than
    /// quietly becoming infinity — the same rule the integer literal
    /// follows, for the same reason.
    pub(crate) fn float_value(&self, tok: Token, negated: bool) -> Result<u64, Diagnostic> {
        let digits: String = self.text(tok).chars().filter(|c| *c != '_').collect();
        let value: f64 = digits.parse().map_err(|_| {
            Diagnostic::new(Rule::LiteralForm, "not a floating-point literal", tok.span)
        })?;
        if !value.is_finite() {
            return Err(Diagnostic::new(
                Rule::LiteralForm,
                "floating-point literal does not fit in `float` (IEEE-754 binary64)",
                tok.span,
            ));
        }
        Ok(if negated { (-value).to_bits() } else { value.to_bits() })
    }

    pub(crate) fn int_value(&self, tok: Token, negated: bool) -> Result<i64, Diagnostic> {
        let text = self.text(tok);
        // The third spelling, and the one whose characters must survive the
        // underscore filter below: `'_'` is a literal, not a digit group
        // (`docs/character-literals.md` §2). The lexer has already checked
        // the shape, so this only decodes.
        if let Some(body) = text.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')) {
            let value = match body.as_bytes() {
                [b'\\', escape] => match escape {
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'0' => 0,
                    other => *other,
                },
                [only] => *only,
                _ => unreachable!("the lexer refuses every other shape"),
            };
            return Ok(if negated { -i64::from(value) } else { i64::from(value) });
        }
        let digits: String = text.chars().filter(|c| *c != '_').collect();
        // A hexadecimal literal is a *spelling*, not a type: `0xff` and
        // `255` are the same node, so `canonical-ast.md` §3 keeps the value
        // and the two hash identically (`docs/bitwise.md` §1.1).
        let magnitude: u64 = match digits.strip_prefix("0x") {
            Some(hex) => u64::from_str_radix(hex, 16),
            None => digits.parse(),
        }
        .map_err(|_| {
            Diagnostic::new(
                Rule::LiteralOutOfRange,
                "integer literal does not fit in `int` (64-bit signed)",
                tok.span,
            )
        })?;
        let limit = if negated { 1u64 << 63 } else { i64::MAX as u64 };
        if magnitude > limit {
            return Err(Diagnostic::new(
                Rule::LiteralOutOfRange,
                "integer literal does not fit in `int` (64-bit signed)",
                tok.span,
            ));
        }
        Ok(if negated { (magnitude as i64).wrapping_neg() } else { magnitude as i64 })
    }
}
