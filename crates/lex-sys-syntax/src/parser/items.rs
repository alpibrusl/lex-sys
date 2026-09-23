//! Items: `module`/`import`, and every top-level declaration a file can
//! hold -- functions, `extern fn`, `struct`, `enum`, `static` -- down to
//! the type and generics surface they share (`type_expr`,
//! `generic_params`, `effect_row`).

use super::*;

impl<'a> Parser<'a> {
    // ---- items ---------------------------------------------------------

    pub(crate) fn unit(&mut self) -> Result<(), Diagnostic> {
        // `module a.b;` is the *first* item in a file, and at most one
        // (`docs/modules.md` §3). "First" is a rule about this file, which
        // is why it is tracked here rather than on the AST: several files
        // parse into one `Ast`, and each gets its own answer.
        let mut declared_module = false;
        let mut seen_item = false;

        while self.peek().kind != TokenKind::Eof {
            match self.peek().kind {
                TokenKind::Module => {
                    let keyword = self.bump();
                    if declared_module {
                        return Err(Diagnostic::new(
                            Rule::ProgramShape,
                            "a file declares at most one module",
                            keyword.span,
                        ));
                    }
                    if seen_item {
                        return Err(Diagnostic::new(
                            Rule::ProgramShape,
                            "a `module` declaration is the first item in its file",
                            keyword.span,
                        ));
                    }
                    let path = self.module_path()?;
                    self.expect(TokenKind::Semi)?;
                    self.current_module = self.ast.module_named(&path);
                    declared_module = true;
                    continue;
                }
                TokenKind::Import => {
                    let keyword = self.bump();
                    let path = self.module_path()?;
                    // The last segment, unless `as` says otherwise (§4).
                    let alias = if self.eat(TokenKind::As) {
                        self.ident()?
                    } else {
                        *path.last().expect("a path has at least one segment")
                    };
                    let end = self.expect(TokenKind::Semi)?.span;
                    let import = Import { path, alias, span: keyword.span.to(end) };
                    self.ast.modules[self.current_module as usize].imports.push(import);
                    seen_item = true;
                    continue;
                }
                _ => {}
            }

            seen_item = true;
            // `pub` is a prefix on a declaration and nothing else, so it is
            // read here and handed down rather than parsed three times.
            let public = self.eat(TokenKind::Pub);
            // `static` is a **contextual** keyword: it is an ordinary
            // identifier everywhere else, because `&static [byte]` has
            // named the region since M3 and making it a token would have
            // to be undone in every type position
            // (`docs/compile-time-data.md` §2).
            if self.peek().kind == TokenKind::Ident && self.text(self.peek()) == "static" {
                self.static_decl(public)?;
                continue;
            }
            match self.peek().kind {
                TokenKind::Fn => self.fn_decl(public)?,
                TokenKind::Extern => self.extern_decl()?,
                TokenKind::Struct => self.struct_decl(None, None, public)?,
                TokenKind::Enum => self.enum_decl(None, None, public)?,
                TokenKind::Res | TokenKind::Val => {
                    let keyword = self.bump();
                    let mode = if keyword.kind == TokenKind::Res { Mode::Res } else { Mode::Val };
                    match self.peek().kind {
                        TokenKind::Struct => {
                            self.struct_decl(Some(mode), Some(keyword.span), public)?
                        }
                        TokenKind::Enum => {
                            self.enum_decl(Some(mode), Some(keyword.span), public)?
                        }
                        other => {
                            return Err(self.err(
                                Rule::TypeMismatch,
                                format!(
                                    "expected `struct` or `enum` after {}, found {}",
                                    keyword.kind.describe(),
                                    other.describe()
                                ),
                            ));
                        }
                    }
                }
                other => {
                    return Err(self.err(
                        Rule::TypeMismatch,
                        format!(
                            "expected `fn`, `extern`, `struct`, `enum` or `static`, found {}",
                            other.describe()
                        ),
                    ));
                }
            };
        }
        Ok(())
    }

    /// `a.b.c` — a module path, segment by segment.
    pub(crate) fn module_path(&mut self) -> Result<Vec<Symbol>, Diagnostic> {
        let mut path = vec![self.ident()?];
        while self.eat(TokenKind::Dot) {
            path.push(self.ident()?);
        }
        Ok(path)
    }

    /// Push a declaration into whichever module this file is in.
    pub(crate) fn push_decl(&mut self, item: Item, span: Span) -> ItemId {
        self.ast.push_item_in(item, span, self.current_module)
    }

    pub(crate) fn fn_decl(&mut self, public: bool) -> Result<ItemId, Diagnostic> {
        let start = self.expect(TokenKind::Fn)?.span;
        let name = self.ident()?;
        let (generics, bounds, regions, outlives) = self.declaration_params()?;

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
        Ok(self.push_decl(
            Item::Fn(FnDecl {
                name,
                public,
                generics,
                bounds,
                regions,
                outlives,
                params,
                effects,
                ret,
                body,
            }),
            start.to(end),
        ))
    }

    /// `static decode_table: [int] { .. }` (`docs/compile-time-data.md` §2).
    ///
    /// A name, a referent type and a body. No parameters, because a
    /// `static` is not called; no effect row, because it performs nothing
    /// by construction and a row nothing performs is decoration
    /// (`linearity-and-effects.md` §7.3); no generics, because there is
    /// nothing to instantiate it at.
    pub(crate) fn static_decl(&mut self, public: bool) -> Result<ItemId, Diagnostic> {
        let start = self.bump().span;
        let name = self.ident()?;
        self.expect(TokenKind::Colon)?;
        let ty = self.type_expr()?;
        let (body, end) = self.block()?;
        Ok(self.push_decl(Item::Static(StaticDecl { name, public, ty, body }), start.to(end)))
    }

    /// `extern fn abs(f: &!r Ffi("libc"), n: int) -> [ffi("libc")] int;`
    ///
    /// A signature and a semicolon. §8.4: the declaration is the only place
    /// a foreign signature is written, the capability is the only way to
    /// reach it, and the row makes the call visible in every caller's
    /// signature all the way up.
    pub(crate) fn extern_decl(&mut self) -> Result<ItemId, Diagnostic> {
        let start = self.expect(TokenKind::Extern)?.span;
        self.expect(TokenKind::Fn)?;
        let name_tok = self.peek();
        let name = self.ident()?;
        let symbol = self.text(name_tok).to_owned();
        // A foreign function is not generic over types -- C has no such
        // thing -- but it is region-polymorphic, because it takes borrowed
        // capabilities like any other function.
        let (generics, _, regions, _) = self.declaration_params()?;
        if let Some(first) = generics.first() {
            let _ = first;
            return Err(self.err(
                Rule::ForeignDeclaration,
                "a foreign function takes no type parameters; C has none to instantiate",
            ));
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
        Ok(self.push_decl(
            Item::Extern(ExternDecl { name, regions, params, effects, ret, symbol }),
            start.to(end),
        ))
    }

    pub(crate) fn struct_decl(
        &mut self,
        mode: Option<Mode>,
        mode_span: Option<Span>,
        public: bool,
    ) -> Result<ItemId, Diagnostic> {
        let start = mode_span.unwrap_or(self.peek().span);
        self.expect(TokenKind::Struct)?;
        let name = self.ident()?;
        let (generics, bounds) = self.generic_params(mode)?;
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
        Ok(self.push_decl(
            Item::Struct(StructDecl { name, public, mode, generics, bounds, fields }),
            start.to(end),
        ))
    }

    pub(crate) fn enum_decl(
        &mut self,
        mode: Option<Mode>,
        mode_span: Option<Span>,
        public: bool,
    ) -> Result<ItemId, Diagnostic> {
        let start = mode_span.unwrap_or(self.peek().span);
        self.expect(TokenKind::Enum)?;
        let name = self.ident()?;
        let (generics, bounds) = self.generic_params(mode)?;
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
        Ok(self.push_decl(
            Item::Enum(EnumDecl { name, public, mode, generics, bounds, variants }),
            start.to(end),
        ))
    }

    /// `[A, B]` or `[A: val, B]` after a declaration's name, or nothing.
    ///
    /// `docs/collections.md` §3: a bound is written where it is not already
    /// implied. A `val` aggregate bounds every parameter by saying `val` --
    /// `val struct Wrap[T]` *is* `val struct Wrap[T: val]` -- so writing it
    /// there is a second way to say one thing. A `res` aggregate implies
    /// nothing about its parameters, and an undeclared one's mode is
    /// computed from them, so in both the bound says something new. That is
    /// what `res struct Vec[T: val]` needs: the vector owns an allocation,
    /// and its elements are copyable because a boxed slice holds `val` data
    /// only.
    pub(crate) fn generic_params(
        &mut self,
        mode: Option<Mode>,
    ) -> Result<(Vec<Symbol>, Vec<Option<Mode>>), Diagnostic> {
        let (generics, bounds, regions, _) = self.declaration_params()?;
        if let Some(region) = regions.first() {
            let _ = region;
            return Err(self.err(
                Rule::RegionMismatch,
                "a type declaration has no region parameters; only a function can take one",
            ));
        }
        if mode == Some(Mode::Val) && bounds.iter().any(Option::is_some) {
            return Err(self.err(Rule::ModeBoundViolated,
                "a `val` declaration already bounds its parameters: `val struct X[T]` means `T` is `val`, so the bound says nothing new; a `res` or undeclared one is where writing it means something",
            ));
        }
        Ok((generics, bounds))
    }

    /// `[io, fs]` between `->` and the return type -- §7's effect row.
    ///
    /// Labels are plain identifiers here. Which ones mean anything is not the
    /// parser's question: a label nothing grounds can never be performed, so
    /// §7.3 refuses it without a registry of legal names.
    pub(crate) fn effect_row(&mut self) -> Result<Vec<EffectLabel>, Diagnostic> {
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
    pub(crate) fn string_literal(&mut self) -> Result<String, Diagnostic> {
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
                'r' => out.push('\r'),
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
    pub(crate) fn declaration_params(
        &mut self,
    ) -> Result<(Vec<Symbol>, Vec<Option<Mode>>, Vec<Symbol>, Vec<(Symbol, Symbol)>), Diagnostic>
    {
        let mut generics = Vec::new();
        let mut bounds = Vec::new();
        let mut regions = Vec::new();
        let mut outlives = Vec::new();
        if self.eat(TokenKind::LBracket) {
            while !matches!(self.peek().kind, TokenKind::RBracket | TokenKind::Where) {
                if self.eat(TokenKind::Amp) {
                    regions.push(self.ident()?);
                } else {
                    generics.push(self.ident()?);
                    // `[T: val]` — this parameter is copyable
                    // (`docs/mode-polymorphism.md` §3.1). There is no
                    // `[T: res]`: unbounded already means "checked as
                    // `res`", so a `res` bound would change nothing (§3.2).
                    bounds.push(if self.eat(TokenKind::Colon) {
                        match self.peek().kind {
                            TokenKind::Val => {
                                self.bump();
                                Some(Mode::Val)
                            }
                            TokenKind::Res => {
                                return Err(self.err(Rule::ModeBoundViolated,
                                    "there is no `res` bound: an unbounded parameter is already checked as `res`, which is the stronger obligation",
                                ));
                            }
                            other => {
                                return Err(self.err(Rule::TypeMismatch, format!(
                                    "expected `val` after `:`, found {}",
                                    other.describe()
                                )));
                            }
                        }
                    } else {
                        None
                    });
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
        Ok((generics, bounds, regions, outlives))
    }

    pub(crate) fn ident(&mut self) -> Result<Symbol, Diagnostic> {
        let tok = self.expect(TokenKind::Ident)?;
        Ok(self.ast.symbols.intern(self.text(tok)))
    }

    /// A written type: a name, optionally applied to arguments.
    ///
    /// The parser does not know which names are types. `int`, `Pair[int, bool]`
    /// and `i32` all parse; only the last is an error, and saying so is the
    /// checker's job.
    pub(crate) fn type_expr(&mut self) -> Result<TypeId, Diagnostic> {
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
        // `(A, B)` — a tuple (`docs/tuples.md`). Unambiguous at the *start*
        // of a type: the only other parenthesis in type position is the one
        // in `Ffi("libc")`, and that follows a name.
        if self.eat(TokenKind::LParen) {
            let mut parts = Vec::new();
            while self.peek().kind != TokenKind::RParen {
                parts.push(self.type_expr()?);
                if !self.eat(TokenKind::Comma) {
                    break;
                }
            }
            let end = self.expect(TokenKind::RParen)?.span;
            return Ok(self.ast.push_type(TypeExpr::Tuple(parts), tok.span.to(end)));
        }
        // `[T]` — a slice's referent. Unambiguous here: a bracket at the
        // *start* of a type can only open one, since a type-argument list
        // follows a name and an effect row follows `->`.
        if self.eat(TokenKind::LBracket) {
            let inner = self.type_expr()?;
            let end = self.expect(TokenKind::RBracket)?.span;
            return Ok(self.ast.push_type(TypeExpr::Slice(inner), tok.span.to(end)));
        }
        // `io.Buffer` — a type reached through an imported module
        // (`docs/modules.md` §4). Unambiguous in type position: nothing
        // else here puts a dot between two names.
        let first = self.ident()?;
        let (qualifier, name) =
            if self.eat(TokenKind::Dot) { (Some(first), self.ident()?) } else { (None, first) };
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
            return Ok(self
                .ast
                .push_type(TypeExpr::Name { name, qualifier, args }, tok.span.to(lit_end)));
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
        Ok(self.ast.push_type(TypeExpr::Name { name, qualifier, args }, tok.span.to(end)))
    }
}
