//! Recursive-descent parser with precedence climbing.
//!
//! The parser refuses on the first error: M0 wants a located refusal, not
//! recovery. Multi-error recovery is M1 work (#1).

use crate::ast::*;
use crate::lexer::{Token, TokenKind, tokenize};
use crate::span::{Diagnostic, Span};

pub fn parse(source: &str) -> Result<Ast, Diagnostic> {
    let tokens = tokenize(source)?;
    let mut p = Parser { source, tokens, pos: 0, ast: Ast::new(), no_struct_literal: false };
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
                TokenKind::Extern => self.extern_decl()?,
                TokenKind::Struct => self.struct_decl(None, None)?,
                TokenKind::Enum => self.enum_decl(None, None)?,
                TokenKind::Res | TokenKind::Val => {
                    let keyword = self.bump();
                    let mode = if keyword.kind == TokenKind::Res { Mode::Res } else { Mode::Val };
                    match self.peek().kind {
                        TokenKind::Struct => self.struct_decl(Some(mode), Some(keyword.span))?,
                        TokenKind::Enum => self.enum_decl(Some(mode), Some(keyword.span))?,
                        other => {
                            return Err(self.err(format!(
                                "expected `struct` or `enum` after {}, found {}",
                                keyword.kind.describe(),
                                other.describe()
                            )));
                        }
                    }
                }
                other => {
                    return Err(self.err(format!(
                        "expected `fn`, `extern`, `struct` or `enum`, found {}",
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
        let (generics, regions, outlives) = self.declaration_params()?;

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

        // M0 has no unit type, so every function states a return type, and
        // §7.2 says it states its effect row too -- `[]` for pure. A type
        // never starts with `[`, so there is nothing to disambiguate.
        self.expect(TokenKind::Arrow)?;
        let effects = self.effect_row()?;
        let ret = self.type_expr()?;

        let (body, end) = self.block()?;
        Ok(self.ast.push_item(
            Item::Fn(FnDecl { name, generics, regions, outlives, params, effects, ret, body }),
            start.to(end),
        ))
    }

    /// `extern fn abs(f: &!r Ffi("libc"), n: int) -> [ffi("libc")] int;`
    ///
    /// A signature and a semicolon. §8.4: the declaration is the only place
    /// a foreign signature is written, the capability is the only way to
    /// reach it, and the row makes the call visible in every caller's
    /// signature all the way up.
    fn extern_decl(&mut self) -> Result<ItemId, Diagnostic> {
        let start = self.expect(TokenKind::Extern)?.span;
        self.expect(TokenKind::Fn)?;
        let name_tok = self.peek();
        let name = self.ident()?;
        let symbol = self.text(name_tok).to_owned();
        // A foreign function is not generic over types -- C has no such
        // thing -- but it is region-polymorphic, because it takes borrowed
        // capabilities like any other function.
        let (generics, regions, _) = self.declaration_params()?;
        if let Some(first) = generics.first() {
            let _ = first;
            return Err(
                self.err("a foreign function takes no type parameters; C has none to instantiate")
            );
        }

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

        self.expect(TokenKind::Arrow)?;
        let effects = self.effect_row()?;
        let ret = self.type_expr()?;
        let end = self.expect(TokenKind::Semi)?.span;
        Ok(self.ast.push_item(
            Item::Extern(ExternDecl { name, regions, params, effects, ret, symbol }),
            start.to(end),
        ))
    }

    fn struct_decl(
        &mut self,
        mode: Option<Mode>,
        mode_span: Option<Span>,
    ) -> Result<ItemId, Diagnostic> {
        let start = mode_span.unwrap_or(self.peek().span);
        self.expect(TokenKind::Struct)?;
        let name = self.ident()?;
        let generics = self.generic_params()?;
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
        Ok(self
            .ast
            .push_item(Item::Struct(StructDecl { name, mode, generics, fields }), start.to(end)))
    }

    fn enum_decl(
        &mut self,
        mode: Option<Mode>,
        mode_span: Option<Span>,
    ) -> Result<ItemId, Diagnostic> {
        let start = mode_span.unwrap_or(self.peek().span);
        self.expect(TokenKind::Enum)?;
        let name = self.ident()?;
        let generics = self.generic_params()?;
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
        Ok(self
            .ast
            .push_item(Item::Enum(EnumDecl { name, mode, generics, variants }), start.to(end)))
    }

    /// `[A, B]` after a declaration's name, or nothing.
    fn generic_params(&mut self) -> Result<Vec<Symbol>, Diagnostic> {
        let (generics, regions, _) = self.declaration_params()?;
        if let Some(region) = regions.first() {
            let _ = region;
            return Err(self
                .err("a type declaration has no region parameters; only a function can take one"));
        }
        Ok(generics)
    }

    /// `[io, fs]` between `->` and the return type -- §7's effect row.
    ///
    /// Labels are plain identifiers here. Which ones mean anything is not the
    /// parser's question: a label nothing grounds can never be performed, so
    /// §7.3 refuses it without a registry of legal names.
    fn effect_row(&mut self) -> Result<Vec<EffectLabel>, Diagnostic> {
        self.expect(TokenKind::LBracket)?;
        let mut effects = Vec::new();
        while self.peek().kind != TokenKind::RBracket {
            let name = self.ident()?;
            // `ffi("libc")` — a label narrowed to a value (§7.4). The value
            // is a literal here and nowhere else, which is what makes the
            // refinement structural.
            let argument = if self.eat(TokenKind::LParen) {
                let text = self.string_literal()?;
                self.expect(TokenKind::RParen)?;
                Some(text)
            } else {
                None
            };
            effects.push(EffectLabel { name, argument });
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RBracket)?;
        Ok(effects)
    }

    /// The text inside a string literal, without its quotes.
    /// The literal's *bytes*, with escapes already resolved.
    ///
    /// Resolved here rather than left for the checker because the AST keeps
    /// values and not spellings (`canonical-ast.md` §3): `"\n"` and a
    /// literal newline would be the same string if one could be written, so
    /// they are the same node.
    fn string_literal(&mut self) -> Result<String, Diagnostic> {
        let tok = self.expect(TokenKind::Str)?;
        let raw = self.text(tok);
        let inner = &raw[1..raw.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            // The lexer already refused anything else.
            match chars.next().expect("the lexer checked the escape") {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                '0' => out.push('\0'),
                other => out.push(other),
            }
        }
        Ok(out)
    }

    /// `[T, &r, &s where s <= r]` after a declaration's name, or nothing.
    ///
    /// A region parameter wears its `&` at the binder (§5.1). The document
    /// writes `fn len[r](s: &r Bytes)` and leaves which is which to be read
    /// off the parameter list; marking the binder means a declaration says so
    /// by itself, and a region parameter nobody used is still a region.
    #[allow(clippy::type_complexity)]
    fn declaration_params(
        &mut self,
    ) -> Result<(Vec<Symbol>, Vec<Symbol>, Vec<(Symbol, Symbol)>), Diagnostic> {
        let mut generics = Vec::new();
        let mut regions = Vec::new();
        let mut outlives = Vec::new();
        if self.eat(TokenKind::LBracket) {
            while !matches!(self.peek().kind, TokenKind::RBracket | TokenKind::Where) {
                if self.eat(TokenKind::Amp) {
                    regions.push(self.ident()?);
                } else {
                    generics.push(self.ident()?);
                }
                if !self.eat(TokenKind::Comma) {
                    break;
                }
            }
            // `where a <= b` — `b` outlives `a` (§5.2). The relation is a
            // stack, so each clause is one pair and there is nothing to solve.
            if self.eat(TokenKind::Where) {
                loop {
                    let inner = self.ident()?;
                    self.expect(TokenKind::LtEq)?;
                    let outer = self.ident()?;
                    outlives.push((inner, outer));
                    if !self.eat(TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.expect(TokenKind::RBracket)?;
        }
        Ok((generics, regions, outlives))
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
        // `&r T` / `&!r T`: the region is named before the referent, so a
        // reference reads left to right as "a reference, valid for r, to T".
        if self.eat(TokenKind::Amp) {
            let unique = self.eat(TokenKind::Bang);
            let region = self.ident()?;
            let inner = self.type_expr()?;
            let end = self.ast.type_span(inner);
            return Ok(self
                .ast
                .push_type(TypeExpr::Ref { unique, region, inner }, tok.span.to(end)));
        }
        // `[T]` — a slice's referent. Unambiguous here: a bracket at the
        // *start* of a type can only open one, since a type-argument list
        // follows a name and an effect row follows `->`.
        if self.eat(TokenKind::LBracket) {
            let inner = self.type_expr()?;
            let end = self.expect(TokenKind::RBracket)?.span;
            return Ok(self.ast.push_type(TypeExpr::Slice(inner), tok.span.to(end)));
        }
        let name = self.ident()?;
        let mut args = Vec::new();
        let mut end = tok.span;
        // `Ffi("libc")` — a type indexed by a literal (§7.4). Parenthesised
        // rather than bracketed because it indexes by a *value*, and the
        // document writes it that way.
        if self.eat(TokenKind::LParen) {
            let text = self.string_literal()?;
            let lit_end = self.expect(TokenKind::RParen)?.span;
            let lit = self.ast.push_type(TypeExpr::Lit(text), tok.span.to(lit_end));
            args.push(lit);
            return Ok(self.ast.push_type(TypeExpr::Name { name, args }, tok.span.to(lit_end)));
        }
        if self.eat(TokenKind::LBracket) {
            while self.peek().kind != TokenKind::RBracket {
                args.push(self.type_expr()?);
                if !self.eat(TokenKind::Comma) {
                    break;
                }
            }
            end = self.expect(TokenKind::RBracket)?.span;
        }
        Ok(self.ast.push_type(TypeExpr::Name { name, args }, tok.span.to(end)))
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

    fn let_stmt(&mut self) -> Result<StmtId, Diagnostic> {
        let kw = self.bump();
        let mutable = kw.kind == TokenKind::Var;
        let name = self.ident()?;

        // `let Point { x, y } = p;` — taking a value apart rather than naming
        // it. Decided by the brace, which cannot otherwise follow a binding.
        if self.peek().kind == TokenKind::LBrace {
            if mutable {
                return Err(self.err(
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
                Stmt::Destructure { struct_name: name, fields, value },
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

    /// `borrow x as &r in { .. }` / `borrow mut x as &!r in { .. }`.
    ///
    /// The `&` and the `!` are written at the binder for the same reason they
    /// are written in a type: a reader should not have to look anywhere else
    /// to know whether this freezes `x` or locks it. The two must agree, so
    /// `borrow mut x as &r` is refused here rather than silently picking one.
    fn borrow_stmt(&mut self) -> Result<StmtId, Diagnostic> {
        let kw = self.bump();
        let unique = self.eat(TokenKind::Mut);
        let value = self.ident()?;
        self.expect(TokenKind::As)?;
        self.expect(TokenKind::Amp)?;
        let bang = self.eat(TokenKind::Bang);
        if bang != unique {
            return Err(self.err(if unique {
                "`borrow mut` binds a unique reference; write `as &!r`"
            } else {
                "`as &!r` binds a unique reference; write `borrow mut`"
            }));
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
    fn region_stmt(&mut self) -> Result<StmtId, Diagnostic> {
        let kw = self.bump();
        let region = self.ident()?;
        let (body, end) = self.block()?;
        Ok(self.ast.push_stmt(Stmt::Region { region, body }, kw.span.to(end)))
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
        loop {
            match self.peek().kind {
                TokenKind::Dot => {
                    self.bump();
                    let tok = self.peek();
                    let name = self.ident()?;
                    let span = self.ast.expr_span(base).to(tok.span);
                    base = self.ast.push_expr(Expr::Field { base, name }, span);
                }
                // `s[i]`. Chains like `.field` does, so `rows[i].len` and
                // `grid[i][j]` need no special case.
                TokenKind::LBracket => {
                    self.bump();
                    let index = self.bracketed(|p| p.expr())?;
                    let end = self.expect(TokenKind::RBracket)?.span;
                    let span = self.ast.expr_span(base).to(end);
                    base = self.ast.push_expr(Expr::Index { base, index }, span);
                }
                _ => return Ok(base),
            }
        }
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
                Item::Struct(_) | Item::Enum(_) | Item::Extern(_) => None,
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
        let Stmt::If { else_block: Some(block), .. } = ast.stmt(decl.body.stmts[0]) else {
            panic!()
        };
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
        let (ast, decl) =
            one_fn("fn f() -> [] int { if (P { b: true }).b { return 1; } return 0; }");
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
        let Stmt::Destructure { struct_name, fields, value } = ast.stmt(decl.body.stmts[0]) else {
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
        let err =
            parse("fn f(x: int) -> [] int { borrow mut x as &r in { return 0; } }").unwrap_err();
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
        let (ast, decl) =
            one_fn("fn f() -> [] int { region a { let n = alloc[a](1); } return 0; }");
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
        let err = parse("fn f() -> [] int { let n = alloc(1); return 0; }")
            .expect_err("should be refused");
        assert!(err.message.contains('['), "{}", err.message);
    }

    #[test]
    fn a_label_may_carry_a_literal() {
        // §7.4: `ffi` and `ffi("libc")` are different labels, and the parser
        // keeps the difference rather than dropping the argument.
        let (ast, decl) = one_fn("fn f() -> [ffi(\"libc\"), io] int { return 0; }");
        let written: Vec<(String, Option<String>)> = decl
            .effects
            .iter()
            .map(|e| (ast.name_of(e.name).to_owned(), e.argument.clone()))
            .collect();
        assert_eq!(
            written,
            vec![("ffi".to_owned(), Some("libc".to_owned())), ("io".to_owned(), None)]
        );
    }

    #[test]
    fn a_foreign_declaration_is_a_signature_with_no_body() {
        // §8.4. It is region-polymorphic like any other function, because
        // the capability that authorises it is borrowed.
        let ast =
            parse("extern fn labs[&f](ffi: &f Ffi(\"libc\"), n: int) -> [ffi(\"libc\")] int;")
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
}
