//! Statements: blocks, `let` and destructuring, places, `borrow` and
//! `region` blocks, `defer`, and `match`.

use crate::*;

impl<'a> FnLowering<'a> {
    pub(crate) fn block(&mut self, block: &Block) -> Result<Vec<Stmt>, Diagnostic> {
        self.scopes.push(Vec::new());
        self.trace.open();
        let out = self.stmts(&block.stmts);
        let events = self.trace.close();
        self.trace.emit(Event::Scope(events));
        self.scopes.pop();
        out
    }

    pub(crate) fn stmts(&mut self, ids: &[StmtId]) -> Result<Vec<Stmt>, Diagnostic> {
        self.defers.push(Vec::new());
        let lowered = self.stmts_in_block(ids);
        let frame = self.defers.pop().expect("pushed above");
        let mut out = lowered?;
        // Falling off the end is an exit, so the frame runs here -- unless
        // the block ended with a `return`, which already ran it on its way
        // out (`docs/defer.md` §2).
        if !terminates(&out) && !frame.is_empty() {
            let expanded = self.expand_defers(&frame)?;
            out.extend(expanded);
        }
        Ok(out)
    }

    pub(crate) fn stmts_in_block(&mut self, ids: &[StmtId]) -> Result<Vec<Stmt>, Diagnostic> {
        let mut out: Vec<Stmt> = Vec::new();
        for (i, &id) in ids.iter().enumerate() {
            if i > 0 && terminates(&out) {
                return Err(Diagnostic::new(
                    Rule::UnreachableStatement,
                    "this statement is unreachable",
                    self.ast.stmt_span(id),
                ));
            }
            out.extend(self.stmt(id)?);
        }
        Ok(out)
    }

    /// One source statement, which may lower to more than one IR statement:
    /// destructuring binds each part separately.
    pub(crate) fn stmt(&mut self, id: StmtId) -> Result<Vec<Stmt>, Diagnostic> {
        if let AstStmt::Destructure { .. } = self.ast.stmt(id) {
            return self.destructure(id);
        }
        if let AstStmt::DestructureTuple { .. } = self.ast.stmt(id) {
            return self.destructure_tuple(id);
        }
        // `defer E;` (`docs/defer.md`) — nothing is lowered *here*. The
        // expression is recorded against this block and expanded at each of
        // its exits, which is what makes this sugar rather than a second set
        // of rules: what the checker and the backend see is the statement a
        // program would have written by hand.
        if let AstStmt::Defer(e) = self.ast.stmt(id) {
            let e = *e;
            self.defers.last_mut().expect("a block is always open").push(e);
            return Ok(Vec::new());
        }
        // A `return` is an exit from *every* block it sits inside, so it runs
        // all the pending frames, innermost first (`docs/defer.md` §2).
        if matches!(self.ast.stmt(id), AstStmt::Return(_))
            && self.defers.iter().any(|frame| !frame.is_empty())
        {
            return self.return_through_defers(id);
        }
        Ok(vec![self.simple_stmt(id)?])
    }

    /// The value of a `return`, checked but **not** yet marked as returned.
    ///
    /// Split out because a pending `defer` runs *between* evaluating this
    /// and the function actually returning, and the trace has to see the
    /// three in that order: the value's reads, then the defers'
    /// consumptions, then the return (`docs/defer.md` §2).
    pub(crate) fn return_value(&mut self, e: ExprId) -> Result<Expr, Diagnostic> {
        let (value, found) = self.expr(e)?;
        // §5 rule 4, at the one place a value can leave a region: a return
        // type names only the function's own region parameters, so a
        // reference into a `borrow` block here is an escape. Checked before
        // the types are compared, because "`r` does not outlive `q`" is a
        // worse way to say it.
        let mut mentioned = Vec::new();
        self.unifier.resolve(&found).regions_into(&mut mentioned);
        if let Some(Region::Block(id)) =
            mentioned.into_iter().find(|r| matches!(r, Region::Block(_)))
        {
            let kind = if self.arenas.contains(&id) { "an arena" } else { "a `borrow` block" };
            return Err(Diagnostic::new(
                Rule::ReferenceEscapesRegion,
                format!(
                    "this returns a reference into `{}`, which is {kind} in this function; a reference may not outlive its region",
                    self.ast.name_of(self.blocks[id as usize].name)
                ),
                self.ast.expr_span(e),
            ));
        }
        let ret = self.ret.clone();
        self.expect_type(&ret, &found, self.ast.expr_span(e))?;
        Ok(value)
    }

    /// `return E;` where a `defer` is pending (`docs/defer.md` §2).
    ///
    /// The value is evaluated **first**, into an unnamed slot, and only then
    /// do the defers run. That order is forced rather than chosen: a
    /// `defer close(f)` alongside `return fd_of(f)` has to read `f` before
    /// the close, and the other order would make every such function
    /// unwritable.
    pub(crate) fn return_through_defers(&mut self, id: StmtId) -> Result<Vec<Stmt>, Diagnostic> {
        let span = self.ast.stmt_span(id);
        let AstStmt::Return(e) = self.ast.stmt(id) else {
            unreachable!("only called for a return");
        };
        let value = self.return_value(*e)?;

        // Into an unnamed slot, so the defers below run after the value has
        // been computed and cannot change what comes back.
        let ty = self.ret.clone();
        let slot = self.temp(ty);
        let mut out = vec![Stmt::Store { place: Place::Slot(slot), value }];

        let frames: Vec<Vec<ExprId>> = self.defers.iter().rev().cloned().collect();
        for frame in frames {
            let expanded = self.expand_defers(&frame)?;
            out.extend(expanded);
        }
        self.trace.emit(Event::Return { span });
        out.push(Stmt::Return(Expr::Load(slot)));
        Ok(out)
    }

    /// Expand one frame of deferred expressions, latest first.
    ///
    /// Each is lowered afresh rather than cloned, because a trace is a
    /// record of what happened on *this* path: lowering it again is what
    /// emits the consumption events on the path being walked, and what makes
    /// a second consumption of the same value the ordinary "already
    /// consumed" error rather than a special case.
    pub(crate) fn expand_defers(&mut self, frame: &[ExprId]) -> Result<Vec<Stmt>, Diagnostic> {
        let mut out = Vec::new();
        for id in frame.iter().rev() {
            let (value, found) = self.expr(*id)?;
            self.trace.emit(Event::Discard {
                ty: found,
                what: "the value this `defer` produced",
                span: self.ast.expr_span(*id),
            });
            out.push(Stmt::Eval(value));
        }
        Ok(out)
    }

    pub(crate) fn simple_stmt(&mut self, id: StmtId) -> Result<Stmt, Diagnostic> {
        let span = self.ast.stmt_span(id);
        Ok(match self.ast.stmt(id) {
            AstStmt::Let { name, mutable, ty, value } => {
                // The initialiser is resolved *before* the binding exists, so
                // `let x = x;` reads the outer `x` or fails, and never itself.
                let (value, found) = self.expr(*value)?;
                let declared = match ty {
                    Some(written) => {
                        let declared = self.written_type(*written)?;
                        self.expect_type(
                            &declared,
                            &found,
                            self.ast.expr_span(match self.ast.stmt(id) {
                                AstStmt::Let { value, .. } => *value,
                                _ => unreachable!(),
                            }),
                        )?;
                        declared
                    }
                    None => found,
                };
                // No check here: shadowing a binding of the same name in
                // this block is allowed exactly when that binding is dead,
                // and whether it is dead is a fact about the trace rather
                // than about the source (`docs/shadowing.md` §4). `declare`
                // records the link; the checker insists.
                let slot = self.declare(*name, declared, *mutable, span);
                Stmt::Store { place: Place::Slot(slot), value }
            }
            AstStmt::Assign { place, value } => {
                let value_span = self.ast.expr_span(*value);
                let (value, found) = self.expr(*value)?;
                // The place is resolved *after* the value, so `x = x + 1`
                // reads `x` before the write is recorded.
                let (place, declared) = self.place(*place, span)?;
                self.expect_type(&declared, &found, value_span)?;
                Stmt::Store { place, value }
            }
            AstStmt::Expr(e) => {
                let (value, found) = self.expr(*e)?;
                self.trace.emit(Event::Discard {
                    ty: found,
                    what: "this value",
                    span: self.ast.expr_span(*e),
                });
                Stmt::Eval(value)
            }
            AstStmt::If { cond, then_block, else_block } => {
                let cond = self.condition(*cond)?;
                self.trace.open();
                let then_body = self.block(then_block)?;
                let then_events = self.trace.close();
                self.trace.open();
                let else_body = match else_block {
                    Some(block) => self.block(block)?,
                    None => Vec::new(),
                };
                let else_events = self.trace.close();
                // An `if` with no `else` still has two arms; the missing one
                // is empty, which is exactly what makes a lone `if` that
                // consumes a value a disagreement (§4.2).
                self.trace.emit(Event::Branch { arms: vec![then_events, else_events], span });
                Stmt::If { cond, then_body, else_body }
            }
            AstStmt::While { cond, body } => {
                // The condition is evaluated before every iteration, so it
                // belongs to the body as far as the back edge is concerned.
                self.trace.open();
                let cond = self.condition(*cond)?;
                let body = self.block(body)?;
                let events = self.trace.close();
                self.trace.emit(Event::Loop { body: events, span });
                Stmt::While { cond, body }
            }
            AstStmt::Match { scrutinee, arms } => self.match_stmt(*scrutinee, arms, span)?,
            AstStmt::Borrow { value, unique, region, body } => {
                self.borrow_stmt(*value, *unique, *region, body, span)?
            }
            AstStmt::Region { region, body } => self.region_stmt(*region, body)?,
            AstStmt::Return(e) => {
                let value = self.return_value(*e)?;
                self.trace.emit(Event::Return { span });
                Stmt::Return(value)
            }
            AstStmt::Defer(_) => unreachable!("handled before the match"),
            AstStmt::Destructure { .. } | AstStmt::DestructureTuple { .. } => {
                unreachable!("handled before the match")
            }
        })
    }

    /// `let File { fd } = f;` — §4.1's third consumer.
    ///
    /// The whole is spent and the parts are produced, each subject to the
    /// rule in turn. Without it a `res` value could never be destroyed: there
    /// is no `drop`, and a type whose parts are all `val` is exactly where an
    /// obligation ends.
    ///
    /// The value is evaluated once into an unnamed slot, so `let P { a, b } =
    /// make();` calls `make` once however many fields it has.
    pub(crate) fn destructure(&mut self, id: StmtId) -> Result<Vec<Stmt>, Diagnostic> {
        let span = self.ast.stmt_span(id);
        let AstStmt::Destructure { struct_name, qualifier, fields, value } = self.ast.stmt(id)
        else {
            unreachable!("only called for a destructuring `let`");
        };
        let (struct_name, qualifier, fields, value_id) =
            (*struct_name, *qualifier, fields.clone(), *value);
        let value_span = self.ast.expr_span(value_id);
        let (value, found) = self.expr(value_id)?;

        let text = self.ast.name_of(struct_name);
        let target = self.target_module(qualifier, span)?;
        let Some(def) = self.defs.iter().find(|d| d.name == struct_name && d.visible_from(target))
        else {
            return Err(Diagnostic::new(
                Rule::NotAStruct,
                format!("`{text}` is not a struct"),
                span,
            ));
        };
        let (def_id, generic_count, public) = (def.def, def.generics.len(), def.public);
        self.check_visible(target, public, "type", struct_name, span)?;
        if released_only(def_id) {
            return Err(Diagnostic::new(
                Rule::CapabilityMisused,
                format!(
                    "`{text}` is a capability and is destroyed by `release`, not by being taken apart; authority is a resource and the function that ends one is named"
                ),
                span,
            ));
        }
        // `docs/file-handles.md` §2: the third thing that owns something
        // the language cannot see. A pattern naming the descriptor would
        // be a way to drop one without `close`, and the kernel keeps that
        // leak rather than the allocator.
        if closed_only(def_id) {
            return Err(Diagnostic::new(
                Rule::LinearValueTakenApart,
                format!(
                    "`{text}` owns an open descriptor and is ended by `file_close`, not by being taken apart"
                ),
                span,
            ));
        }
        // `docs/heap.md` §3: the same rule, for the same reason. What a box
        // owns is an allocation, and a pattern that could name the pointer
        // would be a way to end one without freeing it.
        if unboxed_only(def_id) {
            return Err(Diagnostic::new(
                Rule::LinearValueTakenApart,
                format!(
                    "`{text}` owns an allocation and is ended by `unbox`, not by being taken apart; what it holds comes back out of `unbox`"
                ),
                span,
            ));
        }
        let DefKind::Struct(declared) = &def.kind else {
            return Err(Diagnostic::new(
                Rule::NotAStruct,
                format!("`{text}` is an enum, not a struct; take it apart with `match`"),
                span,
            ));
        };
        let declared = declared.clone();

        let type_args = self.fresh_args(generic_count);
        self.expect_type(&Type::Named(def_id, type_args.clone()), &found, value_span)?;

        if fields.len() != declared.len() {
            return Err(Diagnostic::new(
                Rule::ArityMismatch,
                format!(
                    "`{text}` has {} field{}, but this pattern names {}; destructuring takes the whole value apart",
                    declared.len(),
                    if declared.len() == 1 { "" } else { "s" },
                    fields.len()
                ),
                span,
            ));
        }

        let mut order: Vec<usize> = Vec::with_capacity(fields.len());
        for (position, field) in fields.iter().enumerate() {
            let field_text = self.ast.name_of(*field);
            let Some(index) = declared.iter().position(|(n, _)| n == field) else {
                return Err(Diagnostic::new(
                    Rule::UnknownName,
                    format!("`{text}` has no field `{field_text}`"),
                    span,
                ));
            };
            if order.contains(&index) {
                return Err(Diagnostic::new(
                    Rule::DuplicateDeclaration,
                    format!("field `{field_text}` is named twice"),
                    span,
                ));
            }
            // Shadowing across statements is `docs/shadowing.md` §3 and is
            // checked at replay. Binding a name twice *within one pattern*
            // is a different thing and is still refused below: one pattern,
            // one binding per name (§5).
            if fields[..position].contains(field) {
                return Err(Diagnostic::new(
                    Rule::DuplicateDeclaration,
                    format!("`{field_text}` is bound twice in this pattern"),
                    span,
                ));
            }
            order.push(index);
        }

        let whole = self.temp(Type::Named(def_id, type_args.clone()));
        let mut out = vec![Stmt::Store { place: Place::Slot(whole), value }];
        for (field, index) in fields.iter().zip(order) {
            let ty = declared[index].1.substitute(&type_args, &[]);
            let slot = self.declare(*field, ty, false, span);
            out.push(Stmt::Store {
                place: Place::Slot(slot),
                value: Expr::Field {
                    base: Box::new(Expr::Load(whole)),
                    def: def_id,
                    args: type_args.clone(),
                    index: index as u32,
                },
            });
        }
        Ok(out)
    }

    /// `let (a, b) = t;` (`docs/tuples.md` §3.2).
    ///
    /// The same statement as [`Self::destructure`] against a type with no
    /// declaration, which changes two things and nothing else:
    ///
    /// * The arity comes from the *value*, not from a table. There is no
    ///   declaration to consult, so the pattern is checked against whatever
    ///   the initialiser turned out to be — which means the initialiser's
    ///   type must be known by now, and a bare inference variable is an
    ///   error rather than something to solve from the pattern. A pattern
    ///   is not an annotation.
    /// * The names are the pattern's own. A struct pattern binds field
    ///   names because the fields have names; a tuple has none to inherit,
    ///   so this is the one pattern in the language that may call a
    ///   binding anything — which is `sharing.md` §4's second gap, closed
    ///   by the feature aimed at its first.
    pub(crate) fn destructure_tuple(&mut self, id: StmtId) -> Result<Vec<Stmt>, Diagnostic> {
        let span = self.ast.stmt_span(id);
        let AstStmt::DestructureTuple { names, value } = self.ast.stmt(id) else {
            unreachable!("only called for a tuple destructuring `let`");
        };
        let (names, value_id) = (names.clone(), *value);
        let value_span = self.ast.expr_span(value_id);
        let (value, found) = self.expr(value_id)?;

        let resolved = self.unifier.resolve(&found);
        let Type::Tuple(components) = resolved else {
            return Err(Diagnostic::new(
                Rule::NotATuple,
                format!(
                    "`{}` is not a tuple, so it is not taken apart with `let (..)`",
                    self.unifier.display(&found)
                ),
                value_span,
            ));
        };

        if names.len() != components.len() {
            return Err(Diagnostic::new(
                Rule::ArityMismatch,
                format!(
                    "this tuple has {} components, but the pattern names {}; destructuring takes the whole value apart",
                    components.len(),
                    names.len()
                ),
                span,
            ));
        }

        for (position, name) in names.iter().enumerate() {
            let text = self.ast.name_of(*name);
            // As for a struct pattern: shadowing is §3's rule and is
            // checked at replay; two bindings of one name inside a single
            // pattern is still refused here.
            if names[..position].contains(name) {
                return Err(Diagnostic::new(
                    Rule::DuplicateDeclaration,
                    format!("`{text}` is bound twice in this pattern"),
                    span,
                ));
            }
        }

        // Evaluated once into an unnamed slot, so `let (a, b) = make();`
        // calls `make` once however many components it has.
        let whole = self.temp(Type::Tuple(components.clone()));
        let mut out = vec![Stmt::Store { place: Place::Slot(whole), value }];
        for (index, name) in names.iter().enumerate() {
            let slot = self.declare(*name, components[index].clone(), false, span);
            out.push(Stmt::Store {
                place: Place::Slot(slot),
                value: Expr::TupleField {
                    base: Box::new(Expr::Load(whole)),
                    components: components.clone(),
                    index: index as u32,
                },
            });
        }
        Ok(out)
    }

    /// `borrow x as &r in { .. }` (§5).
    ///
    /// Freezes `x` for the block and binds a reference to it. The region and
    /// the reference share the name `r`, which is how §5 writes it: `r` is
    /// the region in a type and the reference in an expression, and the two
    /// namespaces never meet.
    /// Resolve the left side of an assignment to a place (§5 rule 2).
    ///
    /// A place is a whole local or a field of whatever a *unique* reference
    /// points at, and nothing else. Two things it deliberately is not:
    ///
    /// * a field of an owned local. That is a partial write, and what one
    ///   means for a binding holding a `res` field is a question §4 does not
    ///   answer. Writing the whole value says the same thing and asks
    ///   nothing new.
    /// * anything reached through a *shared* reference. Freezing promises
    ///   the referent will not change; writing through it is that promise
    ///   broken, whoever holds the reference.
    pub(crate) fn place(&mut self, id: ExprId, span: Span) -> Result<(Place, Type), Diagnostic> {
        match self.ast.expr(id) {
            AstExpr::Name(name) => {
                let text = self.ast.name_of(*name);
                let Some(binding) = self.lookup(*name) else {
                    return Err(Diagnostic::new(
                        Rule::UnknownName,
                        format!("`{text}` is not bound here"),
                        span,
                    ));
                };
                if !binding.mutable {
                    return Err(Diagnostic::new(
                        Rule::AssignToImmutable,
                        format!("`{text}` is immutable; declare it with `var` to assign to it"),
                        span,
                    ));
                }
                let (slot, ty) = (binding.slot, binding.ty.clone());
                self.trace.emit(Event::Assign { slot, span });
                Ok((Place::Slot(slot), ty))
            }
            // `*r = e` — replace what a unique reference points at
            // (`docs/reading-references.md` §3).
            AstExpr::Unary { op: ast::UnOp::Deref, operand } => {
                let operand_id = *operand;
                let operand_span = self.ast.expr_span(operand_id);
                let (base_expr, base_ty) = self.expr(operand_id)?;
                let resolved = self.unifier.resolve(&base_ty);
                let Type::Ref { unique, inner, .. } = &resolved else {
                    return Err(Diagnostic::new(
                        Rule::NotAReference,
                        format!(
                            "`{}` is not a reference, so there is nothing for `*` to follow",
                            self.unifier.display(&resolved)
                        ),
                        operand_span,
                    ));
                };
                if !*unique {
                    return Err(Diagnostic::new(Rule::SharedReferenceWritten,
                        "this is a shared reference `&`, which promises its referent will not change; `borrow mut` hands back a unique one"
                            .to_owned(),
                        operand_span,
                    ));
                }
                let referent = self.unifier.resolve(inner);
                // Overwriting a `res` ends it without naming a consumer,
                // which is the silent drop §4 refuses however it is spelled.
                if mode_of(self.defs, self.unifier, &self.bounds, &referent) == Mode::Res {
                    return Err(Diagnostic::new(
                        Rule::LinearValueUnconsumed,
                        format!(
                            "`{}` is `res`, so this would discard a live resource without naming what ends it",
                            self.unifier.display(&referent)
                        ),
                        operand_span,
                    ));
                }
                Ok((Place::Deref { base: base_expr, ty: referent.clone() }, referent))
            }
            AstExpr::Index { base, index } => {
                let (base_id, index_id) = (*base, *index);
                let base_span = self.ast.expr_span(base_id);
                let (base_expr, base_ty) = self.expr(base_id)?;
                let resolved = self.unifier.resolve(&base_ty);
                // A shared slice promises its elements will not change, the
                // same promise a shared reference makes about its referent.
                if let Type::Ref { unique: false, .. } = &resolved {
                    return Err(Diagnostic::new(Rule::SharedReferenceWritten,
                        "this is a shared slice `&`, which promises its elements will not change; `alloc_slice` hands back a unique one"
                            .to_owned(),
                        base_span,
                    ));
                }
                let element = self.element_of(&base_ty, base_span)?;
                let index_span = self.ast.expr_span(index_id);
                let (index_expr, index_ty) = self.expr(index_id)?;
                self.expect_type(&Type::Int, &index_ty, index_span)?;
                Ok((
                    Place::Element { base: base_expr, index: index_expr, element: element.clone() },
                    element,
                ))
            }
            AstExpr::Field { base, name } => {
                let (base_id, field) = (*base, *name);
                let base_span = self.ast.expr_span(base_id);
                let (base_expr, base_ty) = self.expr(base_id)?;
                let resolved = self.unifier.resolve(&base_ty);
                let Type::Ref { unique, inner, .. } = resolved else {
                    return Err(Diagnostic::new(
                        Rule::NotAReference,
                        format!(
                            "`{}` is not a reference, so this would write one field and leave the rest; assign the whole value instead",
                            self.unifier.display(&resolved)
                        ),
                        base_span,
                    ));
                };
                if !unique {
                    return Err(Diagnostic::new(Rule::SharedReferenceWritten,
                        "this is a shared reference `&`, which promises its referent will not change; `borrow mut` binds one that may be written through"
                            .to_owned(),
                        base_span,
                    ));
                }
                let Type::Named(def_id, type_args) = self.unifier.resolve(&inner) else {
                    return Err(Diagnostic::new(
                        Rule::UnknownName,
                        format!("`{}` has no fields", self.unifier.display(&inner)),
                        base_span,
                    ));
                };
                let def =
                    self.defs.iter().find(|d| d.def == def_id).expect("a named type is declared");
                let field_text = self.ast.name_of(field);
                let DefKind::Struct(fields) = &def.kind else {
                    return Err(Diagnostic::new(
                        Rule::LinearValueTakenApart,
                        format!(
                            "`{}` is an enum; its payload is reached by matching, not with `.`",
                            self.ast.name_of(def.name)
                        ),
                        base_span,
                    ));
                };
                let Some(index) = fields.iter().position(|(n, _)| *n == field) else {
                    return Err(Diagnostic::new(
                        Rule::UnknownName,
                        format!("`{}` has no field `{field_text}`", self.ast.name_of(def.name)),
                        span,
                    ));
                };
                let ty = fields[index].1.substitute(&type_args, &[]);
                // The write destroys whatever was there. For a `val` field
                // that costs nothing; for a `res` one it is the silent drop
                // §4 exists to forbid, and there is no consumer named here.
                self.trace.emit(Event::Discard {
                    ty: ty.clone(),
                    what: "the value overwritten here",
                    span,
                });
                Ok((
                    Place::Field {
                        base: base_expr,
                        def: def_id,
                        args: type_args,
                        index: index as u32,
                    },
                    ty,
                ))
            }
            _ => Err(Diagnostic::new(
                Rule::NotAPlace,
                "this cannot be assigned to; a place is a binding or a field reached through a unique reference",
                span,
            )),
        }
    }

    pub(crate) fn borrow_stmt(
        &mut self,
        value: Symbol,
        unique: bool,
        region: Symbol,
        body: &Block,
        span: Span,
    ) -> Result<Stmt, Diagnostic> {
        let text = self.ast.name_of(value);
        let Some(binding) = self.lookup(value) else {
            return Err(Diagnostic::new(
                Rule::UnknownName,
                format!("`{text}` is not bound here"),
                span,
            ));
        };
        let (referent, referent_ty) = (binding.slot, binding.ty.clone());

        let id = self.blocks.len() as u32;
        self.blocks.push(BorrowBlock { name: region, parent: self.open_blocks.last().copied() });
        self.open_blocks.push(id);
        let names: Vec<String> =
            self.blocks.iter().map(|b| self.ast.name_of(b.name).to_owned()).collect();
        self.unifier.set_region_block_names(names);

        self.scopes.push(Vec::new());
        self.trace.open();
        // Frozen for the whole block: not movable, not consumable. A `val`
        // referent notices nothing, because reading one was never a move.
        self.trace.emit(Event::Freeze { slot: referent, unique, span });
        let reference =
            Type::Ref { unique, region: Region::Block(id), inner: Box::new(referent_ty) };
        let reference = self.declare(region, reference, false, span);
        let lowered = self.stmts(&body.stmts);
        self.trace.emit(Event::Thaw { slot: referent, unique });
        let events = self.trace.close();
        self.trace.emit(Event::Scope(events));
        self.scopes.pop();

        self.open_blocks.pop();
        Ok(Stmt::Borrow { referent, reference, unique, body: lowered? })
    }

    /// `region a { .. }` — an arena (§6).
    ///
    /// Almost exactly `borrow_stmt` with the referent taken out, and that is
    /// the section's whole claim made structural: the region it opens is a
    /// block in the same table, so §5.2's outlives relation, §5's
    /// occurs-check and the scope rules all apply to it without a line of
    /// new reasoning. What escapes an arena is decided by the code that
    /// decides what escapes a borrow, because there is only one.
    pub(crate) fn region_stmt(&mut self, region: Symbol, body: &Block) -> Result<Stmt, Diagnostic> {
        let id = self.blocks.len() as u32;
        self.blocks.push(BorrowBlock { name: region, parent: self.open_blocks.last().copied() });
        self.open_blocks.push(id);
        let names: Vec<String> =
            self.blocks.iter().map(|b| self.ast.name_of(b.name).to_owned()).collect();
        self.unifier.set_region_block_names(names);

        // The arena's number is its position among this function's arenas,
        // which is what an `alloc` inside it names at the backend.
        let arena = self.arenas.len() as u32;
        self.arenas.push(id);

        self.scopes.push(Vec::new());
        self.trace.open();
        let lowered = self.stmts(&body.stmts);
        let events = self.trace.close();
        self.trace.emit(Event::Scope(events));
        self.scopes.pop();

        self.open_blocks.pop();
        Ok(Stmt::Region { arena, body: lowered? })
    }

    /// Check a `match`: the scrutinee is an enum, every arm names a variant of
    /// it, no variant is matched twice, and between them the arms cover
    /// everything.
    ///
    /// Exhaustiveness is the point. A `match` that silently did nothing for an
    /// unlisted variant would be a hole in the type system exactly where the
    /// type system is supposed to pay for itself.
    pub(crate) fn match_stmt(
        &mut self,
        scrutinee: ExprId,
        arms: &[ast::MatchArm],
        span: Span,
    ) -> Result<Stmt, Diagnostic> {
        let scrutinee_span = self.ast.expr_span(scrutinee);
        let (value, scrutinee_ty) = self.expr(scrutinee)?;
        let resolved = self.unifier.resolve(&scrutinee_ty);

        // `docs/reading-references.md` §2: a reference gives references.
        // Matching through one binds each payload as a reference into the
        // referent, carrying the scrutinee's mode and its region -- so
        // nothing is moved out, nothing is consumed, and the value is as
        // owned after the match as it was before.
        let (by_reference, borrow) = match &resolved {
            Type::Ref { unique, region, inner } => {
                (true, Some((*unique, *region, self.unifier.resolve(inner))))
            }
            _ => (false, None),
        };
        let matched = match &borrow {
            Some((_, _, inner)) => inner.clone(),
            None => resolved.clone(),
        };
        let Type::Named(def_id, type_args) = matched else {
            return Err(Diagnostic::new(
                Rule::MatchOnANonEnum,
                format!(
                    "`{}` cannot be matched; `match` takes an enum, or a reference to one",
                    self.unifier.display(&resolved)
                ),
                scrutinee_span,
            ));
        };
        let def = self.defs.iter().find(|d| d.def == def_id).expect("a declared type");
        let enum_name = self.ast.name_of(def.name).to_owned();
        let DefKind::Enum(variants) = &def.kind else {
            return Err(Diagnostic::new(
                Rule::MatchOnANonEnum,
                format!("`{enum_name}` is a struct, not an enum; there is nothing to match on"),
                scrutinee_span,
            ));
        };
        let variants = variants.clone();

        let scrutinee_ty = Type::Named(def_id, type_args.clone());
        let mut covered = vec![false; variants.len()];
        let mut wildcard = false;
        let mut lowered: Vec<Arm> = Vec::new();
        let mut arm_events: Vec<Vec<Event>> = Vec::new();

        for arm in arms {
            if wildcard {
                return Err(Diagnostic::new(
                    Rule::MatchArmUnreachable,
                    "this arm is unreachable: `_` above it already matches everything",
                    span,
                ));
            }

            let (variant_index, bindings) = match &arm.pattern {
                ast::Pattern::Wildcard => {
                    if covered.iter().all(|c| *c) {
                        return Err(Diagnostic::new(
                            Rule::MatchArmUnreachable,
                            format!(
                                "this `_` is unreachable: every variant of `{enum_name}` is already matched"
                            ),
                            span,
                        ));
                    }
                    wildcard = true;
                    (None, Vec::new())
                }
                ast::Pattern::Variant { enum_name: written, qualifier, variant, bindings } => {
                    let written_text = self.ast.name_of(*written);
                    // The scrutinee already decides which enum this is, so
                    // the written name is a check rather than a lookup --
                    // and the qualifier is part of the name
                    // (`docs/modules.md` §4). Without it a `match` could
                    // not name an enum another module declares, which made
                    // `std.option` a type a program could hold and never
                    // take apart.
                    let Some(target) = self.ast.resolve_module(self.module, *qualifier) else {
                        return Err(Diagnostic::new(
                            Rule::ModuleNotImported,
                            format!(
                                "`{}` is not an imported module here",
                                self.ast.name_of(qualifier.expect("a `None` qualifier resolves"))
                            ),
                            span,
                        ));
                    };
                    if *written != def.name || !def.visible_from(target) {
                        return Err(Diagnostic::new(
                            Rule::UnknownName,
                            format!(
                                "expected a variant of `{enum_name}`, found one of `{written_text}`"
                            ),
                            span,
                        ));
                    }
                    let variant_text = self.ast.name_of(*variant);
                    let Some(index) = variants.iter().position(|(n, _)| n == variant) else {
                        return Err(Diagnostic::new(
                            Rule::UnknownName,
                            format!("`{enum_name}` has no variant `{variant_text}`"),
                            span,
                        ));
                    };
                    if covered[index] {
                        return Err(Diagnostic::new(
                            Rule::MatchArmUnreachable,
                            format!("`{enum_name}::{variant_text}` is matched twice"),
                            span,
                        ));
                    }
                    let payload = &variants[index].1;
                    if bindings.len() != payload.len() {
                        return Err(Diagnostic::new(
                            Rule::ArityMismatch,
                            format!(
                                "`{enum_name}::{variant_text}` carries {} value{}, but the pattern binds {}",
                                payload.len(),
                                if payload.len() == 1 { "" } else { "s" },
                                bindings.len()
                            ),
                            span,
                        ));
                    }
                    covered[index] = true;
                    (Some(index as u32), bindings.clone())
                }
            };

            // Each arm's bindings live in their own scope, so two arms may
            // bind the same name to different types.
            self.scopes.push(Vec::new());
            self.trace.open();
            let mut slots: Vec<Option<Slot>> = Vec::new();
            if variant_index.is_none() && !by_reference {
                // A `_` arm consumes the scrutinee and never names its parts.
                // For a `val` enum that is a discard and costs nothing; for a
                // `res` one it is the silent drop §4 exists to forbid.
                //
                // Through a reference there is nothing to discard: the match
                // never owned the value, so `_` is simply "look at none of
                // it" (`docs/reading-references.md` §2).
                self.trace.emit(Event::Discard {
                    ty: scrutinee_ty.clone(),
                    what: "the value matched here",
                    span: scrutinee_span,
                });
            }
            if let Some(index) = variant_index {
                // A binding's type comes from the scrutinee's own type
                // arguments: matching `Option[int]` binds an `int`.
                let payload: Vec<Type> = variants[index as usize]
                    .1
                    .iter()
                    .map(|t| {
                        let field = t.substitute(&type_args, &[]);
                        // §2: the scrutinee's mode and region, on every
                        // binding. Several unique references at once is
                        // sound because a variant's payload positions are
                        // *disjoint* -- different offsets in one value, no
                        // two patterns naming the same one (§2.2).
                        match &borrow {
                            Some((unique, region, _)) => Type::Ref {
                                unique: *unique,
                                region: *region,
                                inner: Box::new(field),
                            },
                            None => field,
                        }
                    })
                    .collect();
                for (binding, ty) in bindings.iter().zip(payload) {
                    match binding {
                        Some(name) => {
                            if self.declared_in_current_scope(*name) {
                                self.scopes.pop();
                                self.trace.close();
                                return Err(Diagnostic::new(
                                    Rule::DuplicateDeclaration,
                                    format!(
                                        "`{}` is bound twice in this pattern",
                                        self.ast.name_of(*name)
                                    ),
                                    span,
                                ));
                            }
                            slots.push(Some(self.declare(*name, ty, false, span)));
                        }
                        // `_` still occupies a payload position; it just has
                        // no name, so the backend drops the value — which is
                        // only allowed when there is nothing to drop.
                        None => {
                            // Through a reference the payload was never
                            // owned, so there is nothing here to drop.
                            if !by_reference {
                                self.trace.emit(Event::Discard { ty, what: "this payload", span });
                            }
                            slots.push(None);
                        }
                    }
                }
            }
            let body = self.stmts(&arm.body.stmts);
            let events = self.trace.close();
            arm_events.push(vec![Event::Scope(events)]);
            self.scopes.pop();
            lowered.push(Arm { variant: variant_index, bindings: slots, body: body? });
        }

        if !wildcard && !covered.iter().all(|c| *c) {
            let missing: Vec<String> = covered
                .iter()
                .enumerate()
                .filter(|(_, seen)| !**seen)
                .map(|(i, _)| format!("`{enum_name}::{}`", self.ast.name_of(variants[i].0)))
                .collect();
            return Err(Diagnostic::new(
                Rule::MatchNotExhaustive,
                format!("this `match` does not cover {}", missing.join(", ")),
                span,
            ));
        }

        // A `match` is exhaustive by the check above, so its arms are the
        // whole branch: there is no implicit fall-through arm to join.
        self.trace.emit(Event::Branch { arms: arm_events, span });
        Ok(Stmt::Match {
            scrutinee: value,
            def: def_id,
            args: type_args,
            arms: lowered,
            by_reference,
        })
    }
}
