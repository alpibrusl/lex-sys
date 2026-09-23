//! The walk over a function body: resolution, checking and lowering in
//! one pass (the crate's §13). `FnLowering` is one type, and its `impl`
//! is split by concern across this module's children, which see its
//! private fields.

use crate::*;

mod expr;
mod memory;
mod stmt;

/// A `static` as the checker knows it, before evaluation
/// (`docs/compile-time-data.md` §2).
#[derive(Clone)]
pub(crate) struct StaticDef {
    pub(crate) name: Symbol,
    pub(crate) module: u32,
    /// The referent: `[int]` in `static t: [int] { .. }`.
    pub(crate) referent: Type,
}

/// One `borrow` block, and the block it sits inside.
///
/// The parent link is all §5.2 needs: "`r_inner <= r_outer` holds exactly
/// when `r_outer`'s block lexically encloses `r_inner`'s", which is a walk up
/// this chain. O(depth), no fixpoint, and total because a chain has an end.
pub(crate) struct BorrowBlock {
    pub(crate) name: Symbol,
    pub(crate) parent: Option<u32>,
}

pub(crate) struct Binding {
    pub(crate) name: Symbol,
    pub(crate) slot: Slot,
    pub(crate) ty: Type,
    pub(crate) mutable: bool,
}

pub(crate) struct FnLowering<'a> {
    pub(crate) ast: &'a Ast,
    pub(crate) signatures: &'a [Signature],
    /// Every `static` in the program, in declaration order, so a name that
    /// is not a local can resolve to one (`docs/compile-time-data.md` §2).
    pub(crate) statics: &'a [StaticDef],
    /// Which `static` this body *is*, if it is one. `alloc_slice[static]`
    /// is legal exactly here and nowhere else (§2.1), and a `static` may
    /// read one declared before it and not one declared after — which is
    /// the cheapest rule with no cycles in it.
    pub(crate) lowering_static: Option<u32>,
    pub(crate) externs: &'a [ExternFn],
    pub(crate) defs: &'a [TypeDef],
    pub(crate) unifier: &'a mut Unifier,
    pub(crate) mono: &'a mut Mono,
    /// The names of this function's type parameters, so a written type can
    /// resolve to `Type::Param`...
    pub(crate) generic_names: Vec<Symbol>,
    /// ...and what each was instantiated at, so it can then be substituted.
    pub(crate) generics: Vec<Type>,
    pub(crate) scopes: Vec<Vec<Binding>>,
    pub(crate) slots: Vec<Type>,
    /// How many operators this body folded (`docs/compile-time.md` §2.1).
    pub(crate) folded: usize,
    /// Every effect the body performs, unioned as the walk finds calls
    /// (§7.2: "inside a body there is nothing to infer but a union over the
    /// calls, which is a fold").
    pub(crate) performed: Effects,
    /// The region parameters of this function, so a written `&r` in the body
    /// resolves to the same `Region::Param` the signature used...
    pub(crate) region_params: Vec<(Symbol, Region)>,
    /// ...and the declared `a <= b` pairs, which the body may assume.
    pub(crate) region_outlives: Vec<(u32, u32)>,
    /// Every `borrow` block in this body, in the order they were entered.
    /// `Region::Block(i)` is an index here, so two *sibling* blocks are two
    /// different regions even though they nest to the same depth -- which
    /// depth alone could not tell apart, and §5.2's sibling case is exactly
    /// that.
    pub(crate) blocks: Vec<BorrowBlock>,
    /// The ids of the blocks open right now, outermost first.
    pub(crate) open_blocks: Vec<u32>,
    /// Deferred expressions, one frame per block being lowered, in the order
    /// they were written (`docs/defer.md`).
    ///
    /// A frame is expanded at every exit from its block: falling off the end,
    /// and every `return` in it or under it. In reverse, because a later
    /// `defer` may depend on what an earlier one acquired.
    pub(crate) defers: Vec<Vec<ExprId>>,
    /// The block ids that are *arenas* (§6), in the order they opened. A
    /// block's position here is the number an `alloc` inside it carries to
    /// the backend, and membership is what tells an arena from a borrow's
    /// region -- `alloc[r]` into a borrow's region has nothing to allocate.
    pub(crate) arenas: Vec<u32>,
    /// For each slot, the innermost `borrow` block open when it was declared.
    /// A slot's type may mention that block and its ancestors, and nothing
    /// else: that is §5 rule 4, the escape occurs-check.
    pub(crate) slot_scope: Vec<Option<u32>>,
    /// The name and span each slot was declared with, for that check's
    /// diagnostic. A slot the backend made for itself has no name.
    pub(crate) slot_origin: Vec<(Option<Symbol>, Span)>,
    pub(crate) ret: Type,
    /// What the linearity checker replays once the types are settled
    /// (`linear.rs`). Recorded here because this is where the spans are.
    pub(crate) trace: Trace,
    /// The module this function is in, which is where an unqualified name
    /// resolves (`docs/modules.md` §4).
    pub(crate) module: u32,
    /// This declaration's `val` bounds, for `mode_of` on a `Type::Param`
    /// (`docs/mode-polymorphism.md` §3.1). Empty for a monomorphised
    /// copy, which has no `Param` left to ask about.
    pub(crate) bounds: Vec<Option<Mode>>,
}

impl<'a> FnLowering<'a> {
    pub(crate) fn declare(&mut self, name: Symbol, ty: Type, mutable: bool, span: Span) -> Slot {
        // `docs/shadowing.md` §3. A binding of the same name already in
        // *this* block is shadowed rather than refused, and the link goes
        // into the trace so the checker can insist it was dead. An outer
        // block's binding is not shadowed in this sense: it is still
        // reachable when its own block closes, so the block-close check
        // already covers it (§2.1).
        let shadows = self.shadowed_in_current_scope(name);
        let slot = self.temp(ty.clone());
        self.slot_origin[slot.0 as usize] = (Some(name), span);
        self.scopes.last_mut().expect("a scope is always open").push(Binding {
            name,
            slot,
            ty,
            mutable,
        });
        self.trace.emit(Event::Declare {
            slot,
            name: self.ast.name_of(name).to_owned(),
            span,
            shadows,
        });
        slot
    }

    /// A slot with no name: somewhere for a value to live while its parts are
    /// taken out of it. It carries no linearity obligation of its own, because
    /// consuming the value it was built from already discharged one.
    pub(crate) fn temp(&mut self, ty: Type) -> Slot {
        let slot = Slot(self.slots.len() as u32);
        self.slots.push(ty);
        self.slot_scope.push(self.open_blocks.last().copied());
        self.slot_origin.push((None, Span::new(0, 0)));
        slot
    }

    /// Which module a qualified reference reaches into
    /// (`docs/modules.md` §4), or a located refusal naming the qualifier.
    /// What to do with a folded operator (`docs/compile-time.md` §4).
    ///
    /// A value replaces the node. A trap refuses the program **where the
    /// expression is written**, which §4.1 argues for rather than
    /// assumes: an expression with no value is malformed in the way a
    /// type error is, and making the diagnostic depend on whether the
    /// branch is reachable would put a reachability analysis into the
    /// language. `Unknown` is the ordinary case — the operands were not
    /// both literals — and leaves the node alone.
    pub(crate) fn settle_fold(
        &mut self,
        folded: fold::Folded,
        fallback: Expr,
        span: Span,
    ) -> Result<Expr, Diagnostic> {
        match folded {
            fold::Folded::Value(value) => {
                self.folded += 1;
                Ok(value)
            }
            fold::Folded::Trapped(why) => Err(Diagnostic::new(
                Rule::ConstantTraps,
                format!("{why}; the operands are literals, so this can only trap"),
                span,
            )),
            fold::Folded::Unknown => Ok(fallback),
        }
    }

    /// Which `static` a name refers to, if any. Scoped to the module that
    /// declares it, like every other name (`docs/modules.md` §3).
    pub(crate) fn static_index(&self, name: Symbol) -> Option<u32> {
        let module = self.ast.module_of(ast::ItemId(0));
        let _ = module;
        self.statics.iter().position(|s| s.name == name).map(|i| i as u32)
    }

    pub(crate) fn target_module(
        &self,
        qualifier: Option<Symbol>,
        span: Span,
    ) -> Result<u32, Diagnostic> {
        self.ast.resolve_module(self.module, qualifier).ok_or_else(|| {
            Diagnostic::new(
                Rule::ModuleNotImported,
                format!(
                    "`{}` is not an imported module here; `import` it to reach what is in it",
                    self.ast.name_of(qualifier.expect("resolve only fails with a qualifier"))
                ),
                span,
            )
        })
    }

    /// §5: a declaration in another module has to be `pub`.
    ///
    /// Within one module everything is visible, which is why the root --
    /// where every program written before modules lives -- needs no `pub`
    /// anywhere.
    pub(crate) fn check_visible(
        &self,
        target: u32,
        public: bool,
        what: &str,
        name: Symbol,
        span: Span,
    ) -> Result<(), Diagnostic> {
        if target != self.module && target != PRELUDE_MODULE && !public {
            return Err(Diagnostic::new(
                Rule::NotPublic,
                format!(
                    "{what} `{}` is not `pub`, so it cannot be reached from another module",
                    self.ast.name_of(name)
                ),
                span,
            ));
        }
        Ok(())
    }

    pub(crate) fn lookup(&self, name: Symbol) -> Option<&Binding> {
        self.scopes.iter().rev().find_map(|scope| scope.iter().rev().find(|b| b.name == name))
    }

    pub(crate) fn declared_in_current_scope(&self, name: Symbol) -> bool {
        self.scopes.last().is_some_and(|scope| scope.iter().any(|b| b.name == name))
    }

    /// The slot a new binding of this name would **strand**, if any
    /// (`docs/shadowing.md` §3).
    ///
    /// A shadow strands the binding it covers when the shadow lasts as
    /// long as that binding does — which means the innermost scope, and
    /// nothing outside it. Shadowing from an *inner* block is different
    /// and stays free: the inner block ends first, the outer binding is
    /// reachable again afterwards, and its own block-close check already
    /// covers it (§2.1).
    ///
    /// Parameters are the one place those two scopes are really one.
    /// `scopes[0]` holds the parameters and `scopes[1]` the body's
    /// top-level block; the first has no statements of its own and the two
    /// close together, so a `let` at the top of a body covers a parameter
    /// for the whole of that parameter's life. Without this, shadowing a
    /// live `res` parameter is still caught — by the check at `return` —
    /// but reported on a line where the name means something else (§4.1).
    ///
    /// The *last* matching binding, because shadowing chains: `let n`
    /// three times in a block shadows the second, which shadowed the
    /// first.
    pub(crate) fn shadowed_in_current_scope(&self, name: Symbol) -> Option<Slot> {
        let innermost = self.scopes.len().checked_sub(1)?;
        let also_parameters = usize::from(self.scopes.len() == 2);
        self.scopes[innermost - also_parameters..]
            .iter()
            .rev()
            .find_map(|scope| scope.iter().rev().find(|b| b.name == name))
            .map(|b| b.slot)
    }

    /// A type as written inside this body: resolved against the function's own
    /// type parameters, then substituted with what they were instantiated at.
    ///
    /// Regions in scope are the function's own parameters plus every
    /// `borrow` block open here, innermost last so an inner block shadows an
    /// outer one of the same name.
    pub(crate) fn written_type(&self, id: TypeId) -> Result<Type, Diagnostic> {
        let mut regions = self.region_params.clone();
        for id in &self.open_blocks {
            regions.push((self.blocks[*id as usize].name, Region::Block(*id)));
        }
        let resolved = resolve_type(
            Resolving {
                ast: self.ast,
                defs: self.defs,
                unifier: self.unifier,
                module: self.module,
            },
            Params { names: &self.generic_names, bounds: &self.bounds },
            &regions,
            id,
        )?;
        Ok(resolved.substitute(&self.generics, &[]))
    }

    /// `release(cap)` — end a capability (§8.2).
    ///
    /// Checked here rather than through a written signature because there is
    /// no one type to write: every capability a program can hold by value
    /// ends the same way, and which ones those are is `released_only`'s
    /// answer, not the unifier's. A `Split` is excluded on purpose — taking
    /// one apart is what it is for, so releasing one whole would be
    /// discarding the authority inside it by accident.
    pub(crate) fn release(
        &mut self,
        args: &[ExprId],
        span: Span,
    ) -> Result<(Expr, Type), Diagnostic> {
        let [capability] = args else {
            return Err(Diagnostic::new(
                Rule::ArityMismatch,
                format!("`release` takes 1 argument, but {} were given", args.len()),
                span,
            ));
        };
        let (value, found) = self.expr(*capability)?;
        let resolved = self.unifier.resolve(&found);
        let is_releasable = matches!(&resolved, Type::Named(def, _) if released_only(*def));
        if !is_releasable {
            return Err(Diagnostic::new(
                Rule::CapabilityMisused,
                format!(
                    "`{}` is not a capability and so has nothing to release",
                    self.unifier.display(&resolved)
                ),
                self.ast.expr_span(*capability),
            ));
        }
        Ok((Expr::Call { callee: Callee::Builtin(Builtin::Release), args: vec![value] }, Type::Int))
    }

    /// `narrow(cap, "libc")` — attenuate a capability (§7.4).
    ///
    /// Narrowing is prefix extension, exactly as the document's own example
    /// has it (`Fs("/var")` becomes `Fs("/var/log/app")`). The unnarrowed
    /// root is the empty string, which is a prefix of everything, so the
    /// capability `split` hands out can still become any library — and a
    /// capability already narrowed to one can never become another.
    pub(crate) fn narrow(
        &mut self,
        args: &[ExprId],
        span: Span,
    ) -> Result<(Expr, Type), Diagnostic> {
        let [capability, literal] = args else {
            return Err(Diagnostic::new(
                Rule::ArityMismatch,
                format!("`narrow` takes 2 arguments, but {} were given", args.len()),
                span,
            ));
        };
        let AstExpr::Str(target) = self.ast.expr(*literal) else {
            return Err(Diagnostic::new(
                Rule::CapabilityNotNarrowable,
                "`narrow` takes a literal, so the refinement can be checked where it is written",
                self.ast.expr_span(*literal),
            ));
        };
        let target = target.clone();

        let (value, found) = self.expr(*capability)?;
        let resolved = self.unifier.resolve(&found);
        // Narrowing consumes what it attenuates, which is what makes the
        // wider capability unreachable afterwards. A borrow is precisely the
        // promise to give it back, so there is nothing here to consume.
        if let Type::Ref { inner, .. } = &resolved {
            return Err(Diagnostic::new(
                Rule::CapabilityNotNarrowable,
                format!(
                    "`{}` is borrowed, and narrowing consumes what it attenuates; narrow the capability itself, before lending it",
                    self.unifier.display(inner)
                ),
                span,
            ));
        }
        let Type::Named(def, args) = &resolved else {
            return Err(Diagnostic::new(
                Rule::CapabilityNotNarrowable,
                format!(
                    "`{}` is not a capability and cannot be narrowed",
                    self.unifier.display(&resolved)
                ),
                span,
            ));
        };
        let which = def.0 as usize;
        if which != PRELUDE_FFI && which != PRELUDE_FS {
            return Err(Diagnostic::new(
                Rule::CapabilityNotNarrowable,
                format!(
                    "`{}` carries no value to narrow; `Ffi` and `Fs` are the capabilities that name one",
                    self.unifier.display(&resolved)
                ),
                span,
            ));
        }
        let Some(Type::Lit(current)) = args.first() else {
            return Err(Diagnostic::new(
                Rule::AmbiguousType,
                "cannot tell what this capability was narrowed to; add an annotation",
                span,
            ));
        };
        if !target.starts_with(current.as_str()) {
            return Err(Diagnostic::new(
                Rule::CapabilityNotNarrowable,
                format!(
                    "`{current}` cannot be narrowed to `{target}`: a capability is attenuated, never widened, and a program must not be able to grant itself what it was not given"
                ),
                span,
            ));
        }
        // A path prefix extends at a separator or not at all. `/tmp` is not
        // a prefix of `/tmpevil` in any sense a filesystem would recognise,
        // and a textual check that said otherwise would hand a program the
        // directory next door. `Ffi` has no separator and no such case.
        if which == PRELUDE_FS && !extends_path(current, &target) {
            return Err(Diagnostic::new(
                Rule::CapabilityNotNarrowable,
                format!(
                    "`{current}` cannot be narrowed to `{target}`: a path prefix extends at a `/`, and `{target}` is a different name that merely starts with the same bytes"
                ),
                span,
            ));
        }
        if &target == current {
            return Err(Diagnostic::new(
                Rule::CapabilityNotNarrowable,
                format!("this narrows `{current}` to itself, which grants nothing new"),
                span,
            ));
        }

        Ok((value, Type::Named(DefId(which as u32), vec![Type::Lit(target)])))
    }

    /// The prelude's type ids, in the order `prelude_types` declared them.
    pub(crate) fn prelude(&self) -> Vec<DefId> {
        self.defs[..PRELUDE_COUNT].iter().map(|d| d.def).collect()
    }

    /// Does `outer` outlive `inner` (§5.2)?
    ///
    /// Three cases, and each is a lookup rather than a solve:
    ///
    /// * two blocks — the outer one has the smaller depth, because nesting is
    ///   a stack and a stack is a total order;
    /// * a block against a region parameter — the parameter was open before
    ///   the body started, so it outlives every block in it and no block
    ///   outlives it;
    /// * two parameters — whatever the declaration's `where` clauses say,
    ///   reflexively and transitively.
    ///
    /// O(depth), no fixpoint, and total.
    pub(crate) fn outlives(&self, outer: Region, inner: Region) -> bool {
        match (outer, inner) {
            (a, b) if a == b => true,
            // §4 of `docs/strings.md`: the static region outlives every
            // other, because its data is in the object file rather than in
            // a frame. Nothing outlives it but itself.
            (Region::Static, _) => true,
            (_, Region::Static) => false,
            (Region::Block(a), Region::Block(b)) => self.encloses(a, b),
            (Region::Param(_), Region::Block(_)) => true,
            (Region::Block(_), Region::Param(_)) => false,
            (Region::Param(a), Region::Param(b)) => {
                // Walk the declared pairs from `b` outwards. The set of
                // parameters is tiny and the visited set makes a cyclic
                // `where` terminate rather than being an error of its own.
                let mut stack = vec![b];
                let mut seen = vec![b];
                while let Some(current) = stack.pop() {
                    if current == a {
                        return true;
                    }
                    for (i, o) in &self.region_outlives {
                        if *i == current && !seen.contains(o) {
                            seen.push(*o);
                            stack.push(*o);
                        }
                    }
                }
                false
            }
            // A region variable reaching here means a call site left one
            // unsolved, which `expect_type` reports where it can say more.
            _ => false,
        }
    }

    /// Does block `outer` lexically enclose block `inner`?
    pub(crate) fn encloses(&self, outer: u32, inner: u32) -> bool {
        let mut current = Some(inner);
        while let Some(id) = current {
            if id == outer {
                return true;
            }
            current = self.blocks[id as usize].parent;
        }
        false
    }

    /// The first binding whose type mentions a region it outlives, if any.
    ///
    /// Run once the body is walked and the types are settled: a slot's type
    /// is fixed at its declaration, so the only way it can name a block is if
    /// inference put it there.
    pub(crate) fn escaped_slot(&self) -> Option<(String, String, Span)> {
        for (index, ty) in self.slots.iter().enumerate() {
            let mut mentioned = Vec::new();
            self.unifier.resolve(ty).regions_into(&mut mentioned);
            for region in mentioned {
                let Region::Block(id) = region else { continue };
                if self.in_scope(region, self.slot_scope[index]) {
                    continue;
                }
                let (name, span) = self.slot_origin[index];
                let name =
                    name.map_or_else(|| "a value".to_owned(), |n| self.ast.name_of(n).to_owned());
                return Some((
                    name,
                    self.ast.name_of(self.blocks[id as usize].name).to_owned(),
                    span,
                ));
            }
        }
        None
    }

    /// Is `region` nameable from inside `scope`, the innermost block open
    /// where a slot was declared?
    pub(crate) fn in_scope(&self, region: Region, scope: Option<u32>) -> bool {
        match region {
            Region::Block(id) => scope.is_some_and(|inner| self.encloses(id, inner)),
            // A region parameter is open for the whole body, and the static
            // region is open for the whole program.
            _ => true,
        }
    }

    /// Fresh inference variables, one per type parameter of a declaration.
    pub(crate) fn fresh_args(&mut self, count: usize) -> Vec<Type> {
        (0..count).map(|_| self.unifier.fresh()).collect()
    }

    /// Require `found` to be usable where `expected` is wanted, reporting the
    /// failure at `span`.
    ///
    /// Equality, with two coercions on top and no others.
    ///
    /// §5.2's: a reference whose region *outlives* the expected one is
    /// accepted, because it is valid for at least as long as it needs to be.
    ///
    /// And §6's: a unique reference is accepted where a shared one is
    /// expected, never the reverse. `&!r T` is `&r T` plus permission to
    /// write, so handing one over as read-only gives the callee strictly
    /// less than it already had. Without this, arena data would be
    /// unreachable from every helper written against `&r` — §6 hands back
    /// `&!a` and nothing else, so `value_of[&r](n: &r Node)` could never be
    /// called on anything allocated. The referent stays invariant: "`T`
    /// never changes" is untouched.
    pub(crate) fn expect_type(
        &mut self,
        expected: &Type,
        found: &Type,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let want = self.unifier.shallow(expected);
        let got = self.unifier.shallow(found);
        if let (
            Type::Ref { unique: want_unique, region: want_region, inner: want_inner },
            Type::Ref { unique: got_unique, region: got_region, inner: got_inner },
        ) = (&want, &got)
            && (want_unique == got_unique || (!*want_unique && *got_unique))
        {
            let wanted = self.unifier.resolve_region(*want_region);
            let given = self.unifier.resolve_region(*got_region);
            if wanted.is_var() || given.is_var() {
                // One side is a call site's fresh region: there is nothing to
                // compare yet, so solve it. That is §5.1's instantiation.
                self.unify_regions_at(wanted, given, span)?;
            } else if !self.outlives(given, wanted) {
                return Err(Diagnostic::new(
                    Rule::ReferenceEscapesRegion,
                    format!(
                        "`{}` does not outlive `{}`, so a reference valid for the first cannot be used where the second is expected",
                        self.unifier.display_region(given),
                        self.unifier.display_region(wanted)
                    ),
                    span,
                ));
            }
            let (want_inner, got_inner) = (want_inner.clone(), got_inner.clone());
            return self.expect_exact(&want_inner, &got_inner, span);
        }
        self.expect_exact(&want, &got, span)
    }

    pub(crate) fn unify_regions_at(
        &mut self,
        expected: Region,
        found: Region,
        span: Span,
    ) -> Result<(), Diagnostic> {
        match self.unifier.unify_regions(expected, found) {
            Ok(()) => Ok(()),
            Err(_) => Err(Diagnostic::new(
                Rule::RegionMismatch,
                format!(
                    "`{}` and `{}` are different regions",
                    self.unifier.display_region(expected),
                    self.unifier.display_region(found)
                ),
                span,
            )),
        }
    }

    /// Plain equality, with no coercion anywhere inside.
    pub(crate) fn expect_exact(
        &mut self,
        expected: &Type,
        found: &Type,
        span: Span,
    ) -> Result<(), Diagnostic> {
        match self.unifier.unify(expected, found) {
            Ok(()) => Ok(()),
            Err(UnifyError::Mismatch { expected, found }) => Err(Diagnostic::new(
                Rule::TypeMismatch,
                format!(
                    "expected `{}`, found `{}`",
                    self.unifier.display(&expected),
                    self.unifier.display(&found)
                ),
                span,
            )),
            Err(UnifyError::Infinite { ty, .. }) => Err(Diagnostic::new(
                Rule::InfiniteType,
                format!("this would build an infinite type, `{}`", self.unifier.display(&ty)),
                span,
            )),
            // Two references from different `borrow` blocks, neither of which
            // encloses the other: §5.2's sibling case.
            Err(UnifyError::Regions { expected, found }) => Err(Diagnostic::new(
                Rule::RegionMismatch,
                format!(
                    "`{}` and `{}` are different regions, and neither outlives the other",
                    self.unifier.display_region(expected),
                    self.unifier.display_region(found)
                ),
                span,
            )),
            Err(UnifyError::Uniqueness { expected }) => Err(Diagnostic::new(
                Rule::TypeMismatch,
                if expected {
                    "expected a unique reference `&!`, found a shared one `&`"
                } else {
                    "expected a shared reference `&`, found a unique one `&!`"
                },
                span,
            )),
        }
    }
}

/// What a call's name resolves to — **once**.
///
/// A call used to be resolved twice: once for its effect row and once for
/// its types, with different precedence and neither scoped to a module.
/// Once `docs/modules.md` let two modules hold one name the two answers
/// could differ, and a function calling into C could declare `[]`. One
/// resolution, one answer, and everything reads it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Resolved {
    Builtin(Builtin),
    /// An index into `externs`.
    Extern(usize),
    /// An index into `signatures`.
    Fn(usize),
    None,
}

impl Resolved {
    /// Builtin, then foreign, then written — and the last two only in the
    /// module the call reaches into (`docs/modules.md` §4).
    ///
    /// A builtin is not module-scoped because it is the language rather
    /// than a declaration, exactly as the prelude's types are.
    pub(crate) fn find(f: &FnLowering<'_>, text: &str, callee: Symbol, target: u32) -> Self {
        if let Some(builtin) = Builtin::from_name(text) {
            return Resolved::Builtin(builtin);
        }
        if let Some(index) = f.externs.iter().position(|e| e.name == text && e.module == target) {
            return Resolved::Extern(index);
        }
        match f.signatures.iter().position(|s| s.name == callee && s.module == target) {
            Some(index) => Resolved::Fn(index),
            None => Resolved::None,
        }
    }

    pub(crate) fn effects(self, f: &FnLowering<'_>) -> Effects {
        match self {
            Resolved::Builtin(builtin) => builtin.effects(),
            Resolved::Extern(index) => f.externs[index].effects.clone(),
            Resolved::Fn(index) => f.signatures[index].effects.clone(),
            Resolved::None => Effects::default(),
        }
    }
}

pub(crate) fn bin_op(op: ast::BinOp) -> BinOp {
    match op {
        ast::BinOp::Add => BinOp::Add,
        ast::BinOp::Sub => BinOp::Sub,
        ast::BinOp::Mul => BinOp::Mul,
        ast::BinOp::Div => BinOp::Div,
        ast::BinOp::Rem => BinOp::Rem,
        ast::BinOp::Eq => BinOp::Eq,
        ast::BinOp::Ne => BinOp::Ne,
        ast::BinOp::Lt => BinOp::Lt,
        ast::BinOp::Le => BinOp::Le,
        ast::BinOp::Gt => BinOp::Gt,
        ast::BinOp::Ge => BinOp::Ge,
        ast::BinOp::And => BinOp::And,
        ast::BinOp::Or => BinOp::Or,
        ast::BinOp::BitAnd => BinOp::BitAnd,
        ast::BinOp::BitOr => BinOp::BitOr,
        ast::BinOp::BitXor => BinOp::BitXor,
        ast::BinOp::Shl => BinOp::Shl,
        ast::BinOp::Shr => BinOp::Shr,
    }
}
