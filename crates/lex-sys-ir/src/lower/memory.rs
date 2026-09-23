//! Where values live: arenas, `alloc`, slices and indexing, the heap and
//! `Box`, and files.

use crate::*;

impl<'a> FnLowering<'a> {
    /// `alloc[a](value)` (§6).
    ///
    /// Three questions, and the first two are lookups rather than solves:
    /// which arena `a` names, whether what is being allocated is `val`, and
    /// what type comes back.
    pub(crate) fn alloc(
        &mut self,
        region: Symbol,
        value: ExprId,
        span: Span,
    ) -> Result<(Expr, Type), Diagnostic> {
        // An arena is named, and the name has to be one that is open here.
        // A region *parameter* is not an arena: a caller's region is not
        // this function's to allocate in, and nothing was opened to hold it.
        let id = self.open_arena(region, span)?;

        let value_span = self.ast.expr_span(value);
        let (lowered, ty) = self.expr(value)?;
        let resolved = self.unifier.resolve(&ty);
        // §6.1: an arena releases memory. It does not close files, release
        // capabilities or run anything -- so a `res` value put in one would
        // have its memory reclaimed with its obligation undischarged, which
        // is a leak with a static blessing.
        if mode_of(self.defs, self.unifier, &self.bounds, &resolved) == Mode::Res {
            return Err(Diagnostic::new(
                Rule::ModeBoundViolated,
                format!(
                    "`{}` is `res`, and an arena holds `val` data only: releasing one reclaims memory and runs nothing, so a linear obligation put inside would be dropped rather than discharged",
                    self.unifier.display(&resolved)
                ),
                value_span,
            ));
        }

        let reference = Type::Ref {
            unique: true,
            region: Region::Block(id),
            inner: Box::new(resolved.clone()),
        };
        Ok((
            Expr::Alloc { arena: self.arena_of(id), ty: resolved, value: Box::new(lowered) },
            reference,
        ))
    }

    /// `fs_read(fs, path, into)` and `fs_write(fs, path, bytes)`.
    ///
    /// Checked here rather than through a written signature because the row
    /// depends on what the capability was narrowed to: an `Fs("/tmp")`
    /// performs `fs_read("/tmp")`, and a fixed signature has nowhere to put
    /// the prefix.
    pub(crate) fn file_op(
        &mut self,
        op: Builtin,
        args: &[ExprId],
        span: Span,
    ) -> Result<(Expr, Type), Diagnostic> {
        let [capability, path, buffer] = args else {
            return Err(Diagnostic::new(
                Rule::ArityMismatch,
                format!(
                    "`{}` takes 3 arguments -- the capability, the path and the bytes -- but {} were given",
                    op.name(),
                    args.len()
                ),
                span,
            ));
        };

        let capability_span = self.ast.expr_span(*capability);
        let (fs_value, fs_ty) = self.expr(*capability)?;
        let prefix = self.granted_prefix(&fs_ty, capability_span)?;

        let bytes = Type::Ref {
            unique: false,
            region: self.unifier.fresh_region(),
            inner: Box::new(Type::Slice(Box::new(Type::Byte))),
        };
        let path_span = self.ast.expr_span(*path);
        let (path_value, path_ty) = self.expr(*path)?;
        self.expect_type(&bytes, &path_ty, path_span)?;

        // `fs_read` fills the buffer, so it needs a unique one; `fs_write`
        // only reads what it is handed.
        let buffer_span = self.ast.expr_span(*buffer);
        let (buffer_value, buffer_ty) = self.expr(*buffer)?;
        let wanted = Type::Ref {
            unique: op == Builtin::FsRead,
            region: self.unifier.fresh_region(),
            inner: Box::new(Type::Slice(Box::new(Type::Byte))),
        };
        self.expect_type(&wanted, &buffer_ty, buffer_span)?;

        let label = if op == Builtin::FsRead { "fs_read" } else { "fs_write" };
        self.performed.union(&Effects::new([Label {
            name: label.to_owned(),
            argument: Some(prefix.clone()),
        }]));

        Ok((
            Expr::FileOp {
                write: op == Builtin::FsWrite,
                prefix,
                args: vec![fs_value, path_value, buffer_value],
            },
            Type::Int,
        ))
    }

    /// `open_read(fs, path)` — `docs/file-handles.md` §2.1.
    ///
    /// The same shape as [`Self::file_op`] minus the buffer: the prefix
    /// comes out of the capability's type, the row carries it, and what is
    /// handed back is an `Opened` rather than an `int`, because "the file
    /// or the reason" is two outcomes and `-1` is one sentence
    /// (`file-handles.md` §3).
    pub(crate) fn open_read(
        &mut self,
        args: &[ExprId],
        span: Span,
    ) -> Result<(Expr, Type), Diagnostic> {
        let [capability, path] = args else {
            return Err(Diagnostic::new(
                Rule::ArityMismatch,
                format!("`open_read` takes 2 arguments, but {} were given", args.len()),
                span,
            ));
        };
        let capability_span = self.ast.expr_span(*capability);
        let (fs_value, fs_ty) = self.expr(*capability)?;
        let prefix = self.granted_prefix(&fs_ty, capability_span)?;

        let bytes = Type::Ref {
            unique: false,
            region: self.unifier.fresh_region(),
            inner: Box::new(Type::Slice(Box::new(Type::Byte))),
        };
        let path_span = self.ast.expr_span(*path);
        let (path_value, path_ty) = self.expr(*path)?;
        self.expect_type(&bytes, &path_ty, path_span)?;

        // §4.1: the whole prefix is spent here. `read` performs a path-free
        // label afterwards precisely because this row named the directory.
        self.performed.union(&Effects::new([Label {
            name: "fs_read".to_owned(),
            argument: Some(prefix.clone()),
        }]));

        Ok((
            Expr::OpenFile { prefix, args: vec![fs_value, path_value] },
            Type::Named(self.prelude()[PRELUDE_OPENED], Vec::new()),
        ))
    }

    /// The path prefix a borrowed `Fs` was narrowed to.
    pub(crate) fn granted_prefix(&mut self, ty: &Type, span: Span) -> Result<String, Diagnostic> {
        let resolved = self.unifier.resolve(ty);
        if let Type::Ref { inner, .. } = &resolved
            && let Type::Named(def, args) = self.unifier.resolve(inner)
            && def.0 as usize == PRELUDE_FS
            && let Some(Type::Lit(prefix)) = args.first()
        {
            return Ok(prefix.clone());
        }
        Err(Diagnostic::new(
            Rule::CapabilityMisused,
            format!(
                "`{}` is not a borrowed `Fs`; reading or writing a file is reached through the capability that names the path it may touch",
                self.unifier.display(&resolved)
            ),
            span,
        ))
    }

    /// `box(h, value)` — one value, one allocation (`docs/heap.md` §3).
    pub(crate) fn boxed(
        &mut self,
        args: &[ExprId],
        span: Span,
    ) -> Result<(Expr, Type), Diagnostic> {
        let [heap, value] = args else {
            return Err(Diagnostic::new(
                Rule::ArityMismatch,
                format!(
                    "`box` takes 2 arguments -- the heap and the value -- but {} were given",
                    args.len()
                ),
                span,
            ));
        };
        self.expect_heap(*heap)?;

        let (lowered, ty) = self.expr(*value)?;
        let resolved = self.unifier.resolve(&ty);
        self.performed.union(&Effects::plain(["heap"]));
        Ok((
            Expr::Boxed { ty: resolved.clone(), value: Box::new(lowered) },
            Type::Named(DefId(PRELUDE_BOX as u32), vec![resolved]),
        ))
    }

    /// `unbox(h, b)` — free the allocation and hand the value back (§3).
    ///
    /// The only consumer a box has, which is what makes §3.1 hold: a `Box`
    /// is `res`, so a program that never reaches here for one does not
    /// compile, and the heap cannot leak.
    pub(crate) fn unboxed(
        &mut self,
        args: &[ExprId],
        span: Span,
    ) -> Result<(Expr, Type), Diagnostic> {
        let [heap, boxed] = args else {
            return Err(Diagnostic::new(
                Rule::ArityMismatch,
                format!(
                    "`unbox` takes 2 arguments -- the heap and the box -- but {} were given",
                    args.len()
                ),
                span,
            ));
        };
        self.expect_heap(*heap)?;

        let boxed_span = self.ast.expr_span(*boxed);
        let (lowered, ty) = self.expr(*boxed)?;
        let inner = self.unifier.fresh();
        let wanted = Type::Named(DefId(PRELUDE_BOX as u32), vec![inner.clone()]);
        self.expect_type(&wanted, &ty, boxed_span)?;

        let inner = self.unifier.resolve(&inner);
        // `docs/boxed-slices.md` §3: the two operations are different and
        // have to be. `unbox` hands back what the box held, and `[T]` is
        // unsized -- there is nothing to hand back and no type to hand it
        // back as. Without this the backend received a `Slice` and its own
        // assertion fired, which is a worse way to find out.
        if matches!(inner, Type::Slice(_)) {
            return Err(Diagnostic::new(
                Rule::UnsizedType,
                format!(
                    "`{}` is unsized, so `unbox` has nothing to hand back; `unbox_slice` frees a boxed slice and answers how many elements it freed",
                    self.unifier.display(&inner)
                ),
                boxed_span,
            ));
        }

        self.performed.union(&Effects::plain(["heap"]));
        Ok((Expr::Unboxed { ty: inner.clone(), value: Box::new(lowered) }, inner))
    }

    /// `contents(b)` — the dereference (§3).
    ///
    /// Mode- and region-preserving, which is the whole of its type rule: a
    /// shared borrow of a box yields a shared borrow of what it holds, for
    /// exactly as long. §5 already refuses a reference that outlives its
    /// borrow, so there is nothing here that needed a new rule.
    pub(crate) fn contents(
        &mut self,
        args: &[ExprId],
        span: Span,
    ) -> Result<(Expr, Type), Diagnostic> {
        let [reference] = args else {
            return Err(Diagnostic::new(
                Rule::ArityMismatch,
                format!("`contents` takes 1 argument, but {} were given", args.len()),
                span,
            ));
        };
        let reference_span = self.ast.expr_span(*reference);
        let (lowered, ty) = self.expr(*reference)?;
        let resolved = self.unifier.resolve(&ty);
        if let Type::Ref { unique, region, inner } = &resolved
            && let Type::Named(def, args) = self.unifier.resolve(inner)
            && def.0 as usize == PRELUDE_BOX
            && let Some(element) = args.first()
        {
            let element = self.unifier.resolve(element);
            return Ok((
                Expr::Contents { ty: element.clone(), value: Box::new(lowered) },
                Type::Ref { unique: *unique, region: *region, inner: Box::new(element) },
            ));
        }
        Err(Diagnostic::new(
            Rule::TypeMismatch,
            format!(
                "`{}` is not a borrowed `Box`; `contents` reads what a box holds, so there has to be a box to read",
                self.unifier.display(&resolved)
            ),
            reference_span,
        ))
    }

    /// `*r` — read what a reference points at
    /// (`docs/reading-references.md` §3).
    ///
    /// Copying, so the referent has to be `val`. A `res` behind a reference
    /// is read by borrowing it further or by naming a field; ending one is
    /// the owner's business, and duplicating the obligation is nobody's.
    pub(crate) fn deref(
        &mut self,
        inner: Expr,
        found: &Type,
        span: Span,
    ) -> Result<(Expr, Type), Diagnostic> {
        let resolved = self.unifier.resolve(found);
        let Type::Ref { inner: referent, .. } = &resolved else {
            return Err(Diagnostic::new(
                Rule::NotAReference,
                format!(
                    "`{}` is not a reference, so there is nothing for `*` to follow",
                    self.unifier.display(&resolved)
                ),
                span,
            ));
        };
        let referent = self.unifier.resolve(referent);
        if mode_of(self.defs, self.unifier, &self.bounds, &referent) == Mode::Res {
            return Err(Diagnostic::new(
                Rule::LinearValueTakenApart,
                format!(
                    "`{}` is `res`, so `*` would copy it and leave two values where one obligation is owed; borrow it further or name a field instead",
                    self.unifier.display(&referent)
                ),
                span,
            ));
        }
        Ok((Expr::Deref { ty: referent.clone(), value: Box::new(inner) }, referent))
    }

    /// `box_slice(h, count, fill)` — a run of values on the heap
    /// (`docs/boxed-slices.md` §3).
    pub(crate) fn boxed_slice(
        &mut self,
        args: &[ExprId],
        span: Span,
    ) -> Result<(Expr, Type), Diagnostic> {
        let [heap, count, fill] = args else {
            return Err(Diagnostic::new(
                Rule::ArityMismatch,
                format!(
                    "`box_slice` takes 3 arguments -- the heap, the count and the fill -- but {} were given",
                    args.len()
                ),
                span,
            ));
        };
        self.expect_heap(*heap)?;

        let count_span = self.ast.expr_span(*count);
        let (count_expr, count_ty) = self.expr(*count)?;
        self.expect_type(&Type::Int, &count_ty, count_span)?;

        let fill_span = self.ast.expr_span(*fill);
        let (fill_expr, element) = self.expr(*fill)?;
        let element = self.unifier.resolve(&element);
        // §2.1: the same rule an arena has, for the same reason. Ending a
        // boxed slice frees memory and runs nothing, so a linear obligation
        // inside would be dropped rather than discharged -- and the fill is
        // copied into every element, which a linear value cannot be at all.
        if mode_of(self.defs, self.unifier, &self.bounds, &element) == Mode::Res {
            return Err(Diagnostic::new(
                Rule::ModeBoundViolated,
                format!(
                    "`{}` is `res`, and a boxed slice holds `val` data only: the fill is copied into every element, and a linear value cannot be copied at all",
                    self.unifier.display(&element)
                ),
                fill_span,
            ));
        }

        self.performed.union(&Effects::plain(["heap"]));
        let slice = Type::Slice(Box::new(element.clone()));
        Ok((
            Expr::BoxedSlice { element, count: Box::new(count_expr), fill: Box::new(fill_expr) },
            Type::Named(DefId(PRELUDE_BOX as u32), vec![slice]),
        ))
    }

    /// `unbox_slice(h, b)` — free it, and answer how many elements (§3).
    ///
    /// The only consumer a boxed slice has, which is what keeps
    /// `heap.md` §3.1 true of it: never reaching here is a compile error.
    pub(crate) fn unboxed_slice(
        &mut self,
        args: &[ExprId],
        span: Span,
    ) -> Result<(Expr, Type), Diagnostic> {
        let [heap, boxed] = args else {
            return Err(Diagnostic::new(
                Rule::ArityMismatch,
                format!(
                    "`unbox_slice` takes 2 arguments -- the heap and the box -- but {} were given",
                    args.len()
                ),
                span,
            ));
        };
        self.expect_heap(*heap)?;

        let boxed_span = self.ast.expr_span(*boxed);
        let (lowered, ty) = self.expr(*boxed)?;
        let element = self.unifier.fresh();
        let wanted = Type::Named(DefId(PRELUDE_BOX as u32), vec![Type::Slice(Box::new(element))]);
        self.expect_type(&wanted, &ty, boxed_span)?;

        self.performed.union(&Effects::plain(["heap"]));
        Ok((Expr::UnboxedSlice { value: Box::new(lowered) }, Type::Int))
    }

    /// The `&!x Heap` a heap operation is reached through (§2).
    pub(crate) fn expect_heap(&mut self, heap: ExprId) -> Result<(), Diagnostic> {
        let span = self.ast.expr_span(heap);
        let (_, ty) = self.expr(heap)?;
        // The capability carries no data and so no leaves: what matters here
        // is that `self.expr` ran at all, because that is what recorded the
        // borrow the checker tracks.
        let resolved = self.unifier.resolve(&ty);
        if let Type::Ref { unique: true, inner, .. } = &resolved
            && matches!(self.unifier.resolve(inner), Type::Named(def, _) if def.0 as usize == PRELUDE_HEAP)
        {
            return Ok(());
        }
        Err(Diagnostic::new(
            Rule::CapabilityMisused,
            format!(
                "`{}` is not a uniquely borrowed `Heap`; allocating is reached through the capability that authorises it",
                self.unifier.display(&resolved)
            ),
            span,
        ))
    }

    /// `len(s)` — how many elements a slice has.
    ///
    /// The length travels in the slice itself, so this reads a value that
    /// is already there rather than computing one: a slice is a pointer and
    /// a length, and `len` is the second half.
    pub(crate) fn len(&mut self, args: &[ExprId], span: Span) -> Result<(Expr, Type), Diagnostic> {
        let [slice] = args else {
            return Err(Diagnostic::new(
                Rule::ArityMismatch,
                format!("`len` takes 1 argument, but {} were given", args.len()),
                span,
            ));
        };
        let slice_span = self.ast.expr_span(*slice);
        let (value, ty) = self.expr(*slice)?;
        self.element_of(&ty, slice_span)?;
        Ok((Expr::Len(Box::new(value)), Type::Int))
    }

    /// `alloc_slice[a](count, fill)` — a run of `count` copies of `fill`
    /// (§6, and `defined-behaviour.md` §8's bounds rule).
    ///
    /// The length is a runtime value, which is what makes this a slice
    /// rather than an array: an array's length lives in its type, and a
    /// length in a type is a second kind of generic parameter that M3 is
    /// not buying.
    pub(crate) fn alloc_slice(
        &mut self,
        region: Symbol,
        count: ExprId,
        fill: ExprId,
        span: Span,
    ) -> Result<(Expr, Type), Diagnostic> {
        let id = self.open_arena(region, span)?;

        let count_span = self.ast.expr_span(count);
        let (count_expr, count_ty) = self.expr(count)?;
        self.expect_type(&Type::Int, &count_ty, count_span)?;

        let fill_span = self.ast.expr_span(fill);
        let (fill_expr, element) = self.expr(fill)?;
        let element = self.unifier.resolve(&element);
        // §6.1 again, and for the same reason: an arena reclaims memory and
        // runs nothing, so a linear obligation inside would be dropped
        // rather than discharged. A slice makes it worse -- there would be
        // `count` of them -- but the rule is the one rule, not a new one.
        if mode_of(self.defs, self.unifier, &self.bounds, &element) == Mode::Res {
            return Err(Diagnostic::new(
                Rule::ModeBoundViolated,
                format!(
                    "`{}` is `res`, and an arena holds `val` data only: the fill is copied into every element, and a linear value cannot be copied at all",
                    self.unifier.display(&element)
                ),
                fill_span,
            ));
        }

        let slice = Type::Slice(Box::new(element.clone()));
        // The compile-time arena is not a block and has no number the
        // backend will ever ask for: a `static` body is evaluated, never
        // emitted (`docs/compile-time-data.md` §3).
        let (arena, region) = if id == STATIC_ARENA {
            (STATIC_ARENA, Region::Static)
        } else {
            (self.arena_of(id), Region::Block(id))
        };
        Ok((
            Expr::AllocSlice {
                arena,
                element,
                count: Box::new(count_expr),
                fill: Box::new(fill_expr),
            },
            Type::Ref { unique: true, region, inner: Box::new(slice) },
        ))
    }

    /// `s[i]` — one element, bounds-checked at runtime.
    ///
    /// The check is not optional and not a mode: an out-of-range index
    /// traps, because the alternative is reading past the end of an
    /// allocation, which is the undefined behaviour this language does not
    /// have (`defined-behaviour.md` §1).
    pub(crate) fn index(
        &mut self,
        base: ExprId,
        index: ExprId,
        span: Span,
    ) -> Result<(Expr, Type), Diagnostic> {
        let base_span = self.ast.expr_span(base);
        let (base_expr, base_ty) = self.expr(base)?;
        let element = self.element_of(&base_ty, base_span)?;

        let index_span = self.ast.expr_span(index);
        let (index_expr, index_ty) = self.expr(index)?;
        self.expect_type(&Type::Int, &index_ty, index_span)?;
        let _ = span;

        Ok((
            Expr::Index {
                base: Box::new(base_expr),
                index: Box::new(index_expr),
                element: element.clone(),
            },
            element,
        ))
    }

    /// `s[a..b]` — a half-open run (`docs/slicing.md` §1).
    ///
    /// The result carries the base's region *and its mode*: §4 of that
    /// document is why the unique case stays unique, and it comes down to
    /// `linearity-and-effects.md` §5 already saying that two copies of one
    /// `&!r` alias correctly because there is one buffer behind them.
    /// Overlapping subslices are that, generalised from one address to
    /// several into the same locked buffer.
    pub(crate) fn subslice(
        &mut self,
        base: ExprId,
        start: ExprId,
        end: ExprId,
        span: Span,
    ) -> Result<(Expr, Type), Diagnostic> {
        let base_span = self.ast.expr_span(base);
        let (base_expr, base_ty) = self.expr(base)?;
        let element = self.element_of(&base_ty, base_span)?;

        // The base is a reference to a run; what comes back is the same
        // kind of reference to a shorter one. `element_of` has already
        // refused anything that is not.
        let Type::Ref { unique, region, .. } = self.unifier.resolve(&base_ty) else {
            return Err(Diagnostic::new(
                Rule::NotASlice,
                format!(
                    "`{}` is not a slice, so it has no range to take",
                    self.unifier.display(&base_ty)
                ),
                base_span,
            ));
        };

        let start_span = self.ast.expr_span(start);
        let (start_expr, start_ty) = self.expr(start)?;
        self.expect_type(&Type::Int, &start_ty, start_span)?;
        let end_span = self.ast.expr_span(end);
        let (end_expr, end_ty) = self.expr(end)?;
        self.expect_type(&Type::Int, &end_ty, end_span)?;
        let _ = span;

        Ok((
            Expr::Subslice {
                base: Box::new(base_expr),
                start: Box::new(start_expr),
                end: Box::new(end_expr),
                element: element.clone(),
            },
            Type::Ref { unique, region, inner: Box::new(Type::Slice(Box::new(element))) },
        ))
    }

    /// The element type of whatever `ty` is, if it is a slice at all.
    pub(crate) fn element_of(&mut self, ty: &Type, span: Span) -> Result<Type, Diagnostic> {
        let resolved = self.unifier.resolve(ty);
        if let Type::Ref { inner, .. } = &resolved
            && let Type::Slice(element) = self.unifier.resolve(inner)
        {
            return Ok(self.unifier.resolve(&element));
        }
        Err(Diagnostic::new(
            Rule::NotASlice,
            format!(
                "`{}` is not a slice, so it has no elements to index or take a range of",
                self.unifier.display(&resolved)
            ),
            span,
        ))
    }

    /// The arena `region` names, if it is one open here.
    pub(crate) fn open_arena(&mut self, region: Symbol, span: Span) -> Result<u32, Diagnostic> {
        let text = self.ast.name_of(region);
        // `docs/compile-time-data.md` §2.1: the one way to put something
        // new in the static region, and it is lexical to a `static` item
        // rather than a reachability analysis — a rule a reader can check
        // by looking at one declaration.
        if text == STATIC_REGION {
            if self.lowering_static.is_some() {
                return Ok(STATIC_ARENA);
            }
            return Err(Diagnostic::new(
                Rule::ReferenceEscapesRegion,
                "`alloc` in the `static` region is only legal inside a `static` item;                  elsewhere `static` is read-only data a program cannot add to",
                span,
            ));
        }
        self.open_blocks
            .iter()
            .rev()
            .find(|id| self.arenas.contains(id) && self.blocks[**id as usize].name == region)
            .copied()
            .ok_or_else(|| {
                Diagnostic::new(Rule::RegionNotInScope,
                    format!(
                        "`{text}` is not an arena open here; `alloc` allocates in a `region {text} {{ .. }}` block"
                    ),
                    span,
                )
            })
    }

    /// Which arena number a block id was given.
    pub(crate) fn arena_of(&self, block: u32) -> u32 {
        self.arenas.iter().position(|id| *id == block).expect("an arena block") as u32
    }
}
