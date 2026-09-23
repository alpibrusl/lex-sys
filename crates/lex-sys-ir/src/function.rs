//! One function or `static` at a time: checking a body against its
//! signature, settling its types, and resolving written types.

use crate::*;

/// Replace every inference variable in a lowered body with what it was solved
/// to.
///
/// Types are written into the IR while checking is still in progress, so a
/// node can capture a variable that a later statement settles — `let x:
/// Option[int] = Option::None;` builds the value before the annotation has
/// said what `T` is. One walk afterwards settles them all.
pub(crate) fn settle_types(stmts: &mut [Stmt], unifier: &Unifier) {
    for stmt in stmts {
        match stmt {
            Stmt::Store { place, value } => {
                match place {
                    Place::Field { base, args, .. } => {
                        settle_expr(base, unifier);
                        for arg in args.iter_mut() {
                            *arg = unifier.resolve(arg);
                        }
                    }
                    Place::Deref { base, ty } => {
                        settle_expr(base, unifier);
                        *ty = unifier.resolve(ty);
                    }
                    Place::Element { base, index, element } => {
                        settle_expr(base, unifier);
                        settle_expr(index, unifier);
                        *element = unifier.resolve(element);
                    }
                    Place::Slot(_) => {}
                }
                settle_expr(value, unifier);
            }
            Stmt::Eval(value) | Stmt::Return(value) => settle_expr(value, unifier),
            Stmt::If { cond, then_body, else_body } => {
                settle_expr(cond, unifier);
                settle_types(then_body, unifier);
                settle_types(else_body, unifier);
            }
            Stmt::While { cond, body } => {
                settle_expr(cond, unifier);
                settle_types(body, unifier);
            }
            Stmt::Borrow { body, .. } | Stmt::Region { body, .. } => settle_types(body, unifier),
            Stmt::Match { scrutinee, args, arms, .. } => {
                settle_expr(scrutinee, unifier);
                for arg in args.iter_mut() {
                    *arg = unifier.resolve(arg);
                }
                for arm in arms {
                    settle_types(&mut arm.body, unifier);
                }
            }
        }
    }
}

pub(crate) fn settle_expr(expr: &mut Expr, unifier: &Unifier) {
    match expr {
        Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::Load(_) | Expr::Static(_) => {}
        Expr::Neg(inner) | Expr::Not(inner) | Expr::BitNot(inner) => settle_expr(inner, unifier),
        Expr::Bin { lhs, rhs, .. } => {
            settle_expr(lhs, unifier);
            settle_expr(rhs, unifier);
        }
        Expr::Call { args, .. } => {
            for arg in args {
                settle_expr(arg, unifier);
            }
        }
        Expr::Struct { fields, .. } => {
            for field in fields {
                settle_expr(field, unifier);
            }
        }
        Expr::Alloc { ty, value, .. } => {
            *ty = unifier.resolve(ty);
            settle_expr(value, unifier);
        }
        Expr::Boxed { ty, value } | Expr::Unboxed { ty, value } => {
            *ty = unifier.resolve(ty);
            settle_expr(value, unifier);
        }
        Expr::Contents { ty, value } => {
            *ty = unifier.resolve(ty);
            settle_expr(value, unifier);
        }
        Expr::BoxedSlice { element, count, fill } => {
            *element = unifier.resolve(element);
            settle_expr(count, unifier);
            settle_expr(fill, unifier);
        }
        Expr::UnboxedSlice { value } => settle_expr(value, unifier),
        Expr::Deref { ty, value } => {
            *ty = unifier.resolve(ty);
            settle_expr(value, unifier);
        }
        Expr::AllocSlice { element, count, fill, .. } => {
            *element = unifier.resolve(element);
            settle_expr(count, unifier);
            settle_expr(fill, unifier);
        }
        Expr::Index { base, index, element } => {
            *element = unifier.resolve(element);
            settle_expr(base, unifier);
            settle_expr(index, unifier);
        }
        Expr::Subslice { base, start, end, element } => {
            *element = unifier.resolve(element);
            settle_expr(base, unifier);
            settle_expr(start, unifier);
            settle_expr(end, unifier);
        }
        Expr::Len(inner) => settle_expr(inner, unifier),
        Expr::Bytes(_) => {}
        Expr::FileOp { args, .. } | Expr::OpenFile { args, .. } => {
            for arg in args {
                settle_expr(arg, unifier);
            }
        }
        Expr::FieldRef { base, args, .. } | Expr::FieldAddr { base, args, .. } => {
            settle_expr(base, unifier);
            for arg in args.iter_mut() {
                *arg = unifier.resolve(arg);
            }
        }
        Expr::Field { base, args, .. } => {
            settle_expr(base, unifier);
            for arg in args.iter_mut() {
                *arg = unifier.resolve(arg);
            }
        }
        Expr::Tuple { parts } => {
            for part in parts {
                settle_expr(part, unifier);
            }
        }
        Expr::TupleField { base, components, .. }
        | Expr::TupleFieldRef { base, components, .. }
        | Expr::TupleFieldAddr { base, components, .. } => {
            settle_expr(base, unifier);
            for component in components.iter_mut() {
                *component = unifier.resolve(component);
            }
        }
        Expr::Enum { args, payload, .. } => {
            for arg in args.iter_mut() {
                *arg = unifier.resolve(arg);
            }
            for value in payload {
                settle_expr(value, unifier);
            }
        }
    }
}

/// Check and lower one `static` item's body (`docs/compile-time-data.md` §2).
///
/// A function with no parameters, no generics, no region parameters and a
/// return type of `&static [T]` — so almost everything `lower_function`
/// does has nothing to configure, and what is left is the same body
/// checker, the same linearity trace and the same escape check. A
/// `static` gets no special treatment from any of them, which is the
/// point: `let`, `var`, `while` and `return` mean here what they mean
/// everywhere.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lower_static(
    ast: &Ast,
    defs: &[TypeDef],
    signatures: &[Signature],
    statics: &[StaticDef],
    externs: &[ExternFn],
    unifier: &mut Unifier,
    index: u32,
    item: usize,
    mono: &mut Mono,
) -> Result<Func, Diagnostic> {
    let Item::Static(decl) = &ast.items[item] else {
        unreachable!("a static def always names a static item");
    };
    let module = ast.module_of(ast::ItemId(item as u32));
    let referent = statics[index as usize].referent.clone();
    let ret = Type::Ref { unique: false, region: Region::Static, inner: Box::new(referent) };

    unifier.set_param_names(Vec::new());
    unifier.set_region_param_names(Vec::new());
    unifier.set_region_block_names(Vec::new());

    let mut f = FnLowering {
        ast,
        signatures,
        statics,
        lowering_static: Some(index),
        externs,
        defs,
        unifier,
        mono,
        generic_names: Vec::new(),
        generics: Vec::new(),
        scopes: vec![Vec::new()],
        slots: Vec::new(),
        performed: Effects::pure(),
        folded: 0,
        region_params: Vec::new(),
        region_outlives: Vec::new(),
        blocks: Vec::new(),
        open_blocks: Vec::new(),
        defers: Vec::new(),
        arenas: Vec::new(),
        slot_scope: Vec::new(),
        slot_origin: Vec::new(),
        ret: ret.clone(),
        trace: Trace::new(),
        module,
        bounds: Vec::new(),
    };

    let mut body = f.block(&decl.body)?;
    let mut slots = f.slots.clone();
    let escapes = f.escaped_slot();
    let performed = f.performed.clone();
    let folded = f.folded;
    let trace = f.trace.finish();

    if let Some((name, region, span)) = escapes {
        return Err(Diagnostic::new(
            Rule::ReferenceEscapesRegion,
            format!(
                "`{name}` would hold a reference into `{region}`, which is a `borrow` block it outlives"
            ),
            span,
        ));
    }

    // A `static` has no capability to perform anything with — it has no
    // parameters — so this can only fire if the language grows an effect
    // that needs none. Checked rather than assumed, because that is what
    // §7.3 of `linearity-and-effects.md` asks of every row.
    if let Some(label) = performed.labels().first() {
        return Err(Diagnostic::new(
            Rule::StaticItem,
            format!(
                "`static {}` performs `{}`, and a `static` runs during compilation where there is nothing to perform it on",
                ast.name_of(decl.name),
                label.name
            ),
            ast.item_span(ast::ItemId(item as u32)),
        ));
    }

    settle_types(&mut body, unifier);
    for slot in slots.iter_mut() {
        *slot = unifier.resolve(slot);
    }
    linear::check(defs, unifier, &[], &slots, &trace)?;

    Ok(Func {
        name: ast.name_of(decl.name).to_owned(),
        effects: Effects::pure(),
        performs: Effects::pure(),
        n_params: 0,
        slots,
        ret,
        body,
        folded,
        span: ast.item_span(ast::ItemId(item as u32)),
    })
}

/// Check and lower one function at one instantiation.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lower_function(
    ast: &Ast,
    defs: &[TypeDef],
    signatures: &[Signature],
    statics: &[StaticDef],
    externs: &[ExternFn],
    unifier: &mut Unifier,
    index: usize,
    args: &[Type],
    mono: &mut Mono,
) -> Result<Func, Diagnostic> {
    let signature = &signatures[index];
    let Item::Fn(decl) = &ast.items[signature.item] else {
        unreachable!("a signature always names a function");
    };

    // Regions are *not* substituted here and a function is not copied per
    // region: a reference is a pointer and has no idea which block it came
    // from, so there is nothing to specialise. Region parameters stay rigid
    // inside the body and are instantiated at each call site instead (§5.1).
    let params: Vec<Type> = signature.params.iter().map(|t| t.substitute(args, &[])).collect();
    let ret = signature.ret.substitute(args, &[]);

    // So a diagnostic inside this body says `T` rather than `T0`.
    unifier
        .set_param_names(signature.generics.iter().map(|g| ast.name_of(*g).to_owned()).collect());
    unifier.set_region_param_names(
        signature.regions.iter().map(|g| ast.name_of(*g).to_owned()).collect(),
    );
    // Every `borrow` block in this body gets its name here as it is entered;
    // the list is indexed by block id and never shrinks, so a diagnostic can
    // still name a region whose block has closed -- which is exactly the
    // case an escape diagnostic has to talk about.
    unifier.set_region_block_names(Vec::new());

    let mut f = FnLowering {
        ast,
        signatures,
        statics,
        lowering_static: None,
        externs,
        defs,
        unifier,
        mono,
        generic_names: signature.generics.clone(),
        generics: args.to_vec(),
        scopes: vec![Vec::new()],
        slots: Vec::new(),
        performed: Effects::pure(),
        folded: 0,
        region_params: signature
            .regions
            .iter()
            .enumerate()
            .map(|(i, name)| (*name, Region::Param(i as u32)))
            .collect(),
        region_outlives: signature.outlives.clone(),
        blocks: Vec::new(),
        open_blocks: Vec::new(),
        defers: Vec::new(),
        arenas: Vec::new(),
        slot_scope: Vec::new(),
        slot_origin: Vec::new(),
        ret: ret.clone(),
        trace: Trace::new(),
        module: signature.module,
        // A copy being emitted has its parameters substituted away, so
        // only the rigid check of pass 1 has bounds to consult.
        bounds: if args.iter().any(|a| matches!(a, Type::Param(_))) || args.is_empty() {
            signature.bounds.clone()
        } else {
            Vec::new()
        },
    };

    for (param, ty) in decl.params.iter().zip(params.iter()) {
        // Parameters are immutable: the shape of a binding handed to you, not
        // one you own outright. A `res` parameter is live from entry, and the
        // body owes exactly one consumption of it on every path.
        f.declare(param.name, ty.clone(), false, ast.type_span(param.ty));
    }
    let mut body = f.block(&decl.body)?;
    let bounds = f.bounds.clone();
    let mut slots = f.slots.clone();
    let escapes = f.escaped_slot();
    let performed = f.performed.clone();
    let trace = f.trace.finish();

    // §5 rule 4, over every binding rather than only the ones that return: a
    // slot's type may name a `borrow` block only if that block was open when
    // the slot was declared. One traversal of one type per slot, which is
    // what "escape is an occurs-check" buys.
    if let Some((name, region, span)) = escapes {
        return Err(Diagnostic::new(
            Rule::ReferenceEscapesRegion,
            format!(
                "`{name}` would hold a reference into `{region}`, which is a `borrow` block it outlives"
            ),
            span,
        ));
    }

    // Read before `f`'s borrow of the unifier ends with it.
    let folded = f.folded;

    settle_types(&mut body, unifier);
    for slot in slots.iter_mut() {
        *slot = unifier.resolve(slot);
    }

    // A slot whose type never got settled means the program did not say
    // enough. Better to name it here than to hand the backend a type that is
    // still a question.
    if let Some(unsettled) = slots.iter().find(|ty| ty.has_var()) {
        let _ = unsettled;
        return Err(Diagnostic::new(
            Rule::AmbiguousType,
            format!(
                "cannot tell what type a binding in `{}` has; add an annotation",
                ast.name_of(decl.name)
            ),
            ast.item_span(ast::ItemId(signature.item as u32)),
        ));
    }

    // Linearity runs last, on settled types: a mode is a fact about a type,
    // and a type is not a fact until inference is done (`linear.rs`).
    //
    // A generic body is checked once with its parameters rigid, where an
    // unbounded parameter is **`res`** and a `[T: val]` one is `val`
    // (`docs/mode-polymorphism.md` §3.1), and then again per
    // instantiation, where it is whatever it was instantiated at.
    //
    // Assuming `res` for the unbounded case is what moves the error to the
    // definition: `res` is the stronger obligation, so a body that passes
    // the rigid check is safe at every instantiation, and one that does
    // not is wrong where it is written rather than wherever somebody
    // first used a resource type. §4 is the trade in full. An
    // instantiation error is still possible -- a type mismatch, say -- so
    // it is still named.
    if let Err(error) = linear::check(defs, unifier, &bounds, &slots, &trace) {
        // Pass 1 checks a generic body with its own parameters standing in
        // for themselves, so "instantiated at `T`" would be a confusing
        // way to describe the definition being checked as written. Only a
        // real instantiation -- one with a concrete argument -- is named.
        let rigid = args.iter().all(|a| matches!(a, Type::Param(_)));
        if args.is_empty() || rigid {
            return Err(error);
        }
        let at: Vec<String> = args.iter().map(|a| unifier.display(a)).collect();
        // The rule is the inner one: this adds *where* the body was being
        // checked, and says nothing about which rule was broken
        // (`docs/agent-errors.md` §3).
        return Err(Diagnostic::new(
            error.rule,
            format!(
                "{} (checking `{}` instantiated at `{}`)",
                error.message,
                ast.name_of(decl.name),
                at.join("`, `")
            ),
            error.span,
        ));
    }

    // §8.2: a row lists the capabilities a function *borrows*. Owning one
    // is strictly stronger and strictly more visible -- it is right there in
    // the parameter list -- so an effect the function has the authority for
    // outright does not appear in its row. That is why `main(world: World)`
    // has row `[]` while printing: it owns the authority rather than
    // borrowing it.
    //
    // Taken from the signature, not from the body, so the rule lives at the
    // boundary with every other rule.
    let mut performed = performed;
    // Kept before the discharge below, because this is the only place the
    // full set exists: `main` owns its capabilities, so discharging leaves
    // `[]` behind however much the body did (`docs/authority.md` §2).
    let performs = performed.clone();
    let mut authority = Effects::pure();
    for param in &params {
        authority.union(&discharged_by(defs, param));
    }
    performed.discharge(&authority);

    // §7.3: the row is exact, or it is decoration. Both directions are
    // errors, and the over-wide one is not a warning -- an inexact row means
    // `[]` no longer means pure, which costs examples-as-tests and costs a
    // signature hash that means anything.
    let declared = &signature.effects;
    if let Some(label) = performed.missing_from(declared) {
        return Err(Diagnostic::new(
            Rule::EffectNotDeclared,
            format!(
                "`{}` performs `{label}`, which its row {declared} does not declare; narrow the body or widen the row",
                ast.name_of(decl.name)
            ),
            ast.item_span(ast::ItemId(signature.item as u32)),
        ));
    }
    if let Some(label) = declared.missing_from(&performed) {
        return Err(Diagnostic::new(
            Rule::EffectDeclaredNotPerformed,
            format!(
                "`{}` declares `{label}` but never performs it; a row is exact or it is decoration",
                ast.name_of(decl.name)
            ),
            ast.item_span(ast::ItemId(signature.item as u32)),
        ));
    }

    if !terminates(&body) {
        return Err(Diagnostic::new(
            Rule::MissingReturn,
            format!("function `{}` can finish without returning a value", ast.name_of(decl.name)),
            ast.item_span(ast::ItemId(signature.item as u32)),
        ));
    }

    Ok(Func {
        name: instance_name(ast.name_of(decl.name), args, unifier),
        effects: signature.effects.clone(),
        performs,
        n_params: decl.params.len() as u32,
        slots,
        ret,
        body,
        folded,
        span: ast.item_span(ast::ItemId(signature.item as u32)),
    })
}

/// A declaration's region parameters must be distinct and must not collide
/// with its type parameters, returning the scope a type in the signature is
/// resolved against.
///
/// Regions and types live in separate namespaces as far as the checker is
/// concerned -- one can never be written where the other is expected -- but
/// letting `fn f[T, &T]` through would make every diagnostic about it a
/// riddle, so it is refused.
pub(crate) fn check_region_names(
    ast: &Ast,
    regions: &[Symbol],
    generics: &[Symbol],
    span: Span,
) -> Result<Vec<(Symbol, Region)>, Diagnostic> {
    let mut scope = Vec::with_capacity(regions.len());
    for (index, name) in regions.iter().enumerate() {
        let text = ast.name_of(*name);
        if scope.iter().any(|(seen, _)| seen == name) {
            return Err(Diagnostic::new(
                Rule::DuplicateDeclaration,
                format!("region parameter `{text}` is declared twice"),
                span,
            ));
        }
        if generics.contains(name) {
            return Err(Diagnostic::new(
                Rule::RegionMismatch,
                format!("`{text}` is both a type parameter and a region parameter here"),
                span,
            ));
        }
        if text == STATIC_REGION {
            return Err(Diagnostic::new(
                Rule::StaticItem,
                "`static` is the region a program's literals live in; it cannot be declared",
                span,
            ));
        }
        scope.push((*name, Region::Param(index as u32)));
    }
    Ok(scope)
}

/// A declaration's type parameters must be distinct and must not shadow a
/// built-in type name.
pub(crate) fn check_generic_names(
    ast: &Ast,
    generics: &[Symbol],
    span: Span,
) -> Result<(), Diagnostic> {
    let mut seen: Vec<Symbol> = Vec::new();
    for name in generics {
        let text = ast.name_of(*name);
        if matches!(text, "int" | "bool") {
            return Err(Diagnostic::new(
                Rule::BuiltinRedeclared,
                format!("`{text}` is a built-in type and cannot be a type parameter"),
                span,
            ));
        }
        if seen.contains(name) {
            return Err(Diagnostic::new(
                Rule::DuplicateDeclaration,
                format!("type parameter `{text}` is declared twice"),
                span,
            ));
        }
        seen.push(*name);
    }
    Ok(())
}

/// Turn a written type into a real one.
///
/// `generics` is the enclosing declaration's type parameters; a written name
/// matching one of them is that parameter rather than a lookup. Parameters
/// shadow nothing else, because a declaration that named one `int` was already
/// refused.
/// What resolving a written type needs to know, besides the type itself.
///
/// These four always travel together -- the program, its declarations,
/// the unifier that renders them, and the module a name is looked up in
/// (`docs/modules.md` §4) -- so they travel as one thing.
#[derive(Clone, Copy)]
pub(crate) struct Resolving<'a> {
    pub(crate) ast: &'a Ast,
    pub(crate) defs: &'a [TypeDef],
    pub(crate) unifier: &'a Unifier,
    pub(crate) module: u32,
}

/// The type parameters of the declaration a type is written *inside*: their
/// names, and the `val` bound each carries.
///
/// Two parallel slices because `Type::Param(i)` indexes both, and together
/// rather than separately because the bound is only ever needed where the
/// name is. Threading it is not bookkeeping: a bound on the enclosing
/// declaration is what makes `Wrap[T]` legal inside `fn f[T: val]`, and
/// reading the names without the bounds is what made it illegal
/// (`docs/collections.md` §4).
#[derive(Clone, Copy)]
pub(crate) struct Params<'a> {
    pub(crate) names: &'a [Symbol],
    pub(crate) bounds: &'a [Option<Mode>],
}

impl<'a> Params<'a> {
    /// A declaration that takes no type parameters, or one whose parameters
    /// are all unbounded.
    pub(crate) fn unbounded(names: &'a [Symbol]) -> Self {
        Params { names, bounds: &[] }
    }
}

pub(crate) fn resolve_type(
    cx: Resolving<'_>,
    params: Params<'_>,
    regions: &[(Symbol, Region)],
    id: TypeId,
) -> Result<Type, Diagnostic> {
    // Sized by default: `[T]` is a referent, and the one caller that may
    // have one is the reference that points at it.
    resolve_type_at(cx, params, regions, id, false)
}

pub(crate) fn resolve_type_at(
    cx: Resolving<'_>,
    params: Params<'_>,
    regions: &[(Symbol, Region)],
    id: TypeId,
    unsized_ok: bool,
) -> Result<Type, Diagnostic> {
    let Resolving { ast, defs, unifier, module } = cx;
    let generics = params.names;
    let span = ast.type_span(id);

    // `&r T`: the region must already be in scope. A name that is not a
    // region parameter of this declaration and not a `borrow` block open
    // around this type is simply not a region, which is the first half of
    // §5's escape example -- `fn escape(f: File) -> &r File` names an `r`
    // that exists nowhere.
    if let TypeExpr::Ref { unique, region, inner } = ast.ty(id) {
        let text = ast.name_of(*region);
        // `&static [byte]` — the one region with a name rather than a
        // binder (`docs/strings.md` §4). It outlives everything, so a
        // function may hand a literal back to any caller; without a way to
        // write it, a literal could not leave the function that wrote it,
        // which is not a restriction anything is buying.
        if text == STATIC_REGION {
            if *unique {
                return Err(Diagnostic::new(
                    Rule::StaticItem,
                    "`static` holds a program's literals, which are shared; there is no unique reference into it",
                    span,
                ));
            }
            return Ok(Type::Ref {
                unique: false,
                region: Region::Static,
                inner: Box::new(resolve_type_at(cx, params, regions, *inner, true)?),
            });
        }
        let Some((_, found)) = regions.iter().rev().find(|(name, _)| name == region) else {
            return Err(Diagnostic::new(
                Rule::RegionNotInScope,
                format!(
                    "`{text}` is not a region in scope; a region comes from a `[&{text}]` parameter or a `borrow` block"
                ),
                span,
            ));
        };
        return Ok(Type::Ref {
            unique: *unique,
            region: *found,
            // The one place an unsized referent is allowed: `&r [T]` is how
            // a slice is written, and the reference is what gives it a size.
            inner: Box::new(resolve_type_at(cx, params, regions, *inner, true)?),
        });
    }

    // `Ffi("libc")`: a literal stands where a type argument does, because
    // what a capability is narrowed to is part of its type (§7.4). It names
    // nothing to look up and has no arguments of its own.
    if let TypeExpr::Lit(text) = ast.ty(id) {
        return Ok(Type::Lit(text.clone()));
    }

    // `[T]`: a shape rather than a name, so there is nothing to look up.
    // It is unsized, which is why it only ever appears under a reference --
    // and that is checked where a type is *used*, not here.
    if let TypeExpr::Slice(inner) = ast.ty(id) {
        // `[T]` has no size of its own -- that is what the length in a
        // slice is for -- so it cannot be a parameter, a field, a return
        // type or a type argument. Only a reference may point at one.
        if !unsized_ok {
            return Err(Diagnostic::new(
                Rule::UnsizedType,
                "`[T]` has no size of its own, so it cannot be used as a value; write `&r [T]` or `&!r [T]`, which is a slice",
                span,
            ));
        }
        let element = resolve_type(cx, params, regions, *inner)?;
        return Ok(Type::Slice(Box::new(element)));
    }

    // `(A, B)`: structural, so there is nothing to look up here either
    // (`docs/tuples.md` §2.2). The arity check is here, not in the parser:
    // `()` and a trailing comma, `(T,)`, both parse as tuples, and both are
    // refused here rather than silently becoming a unit type or a one-tuple
    // nobody asked for (§2.1).
    if let TypeExpr::Tuple(parts) = ast.ty(id) {
        let parts = parts.clone();
        if parts.len() < 2 {
            return Err(Diagnostic::new(
                Rule::PatternShape,
                "a tuple has two components or more; `(T)` is grouping, and neither `(T,)` nor `()` is a tuple",
                span,
            ));
        }
        let components = parts
            .iter()
            .map(|part| resolve_type(cx, params, regions, *part))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(Type::Tuple(components));
    }

    let TypeExpr::Name { name: written_name, qualifier: written_qualifier, args: written_args } =
        ast.ty(id)
    else {
        unreachable!("a reference, a literal, a slice and a tuple were handled above");
    };
    let (written_name, written_qualifier, written_args) =
        (*written_name, *written_qualifier, written_args.clone());
    let name = ast.name_of(written_name);

    // `docs/modules.md` §4: a qualifier says which module to look in, and
    // an unqualified name means this one. A qualifier that no `import`
    // bound is an error here rather than a silent fall back to the local
    // module -- falling back is how a typo becomes a different program.
    let Some(target) = ast.resolve_module(module, written_qualifier) else {
        return Err(Diagnostic::new(
            Rule::ModuleNotImported,
            format!(
                "`{}` is not an imported module here; `import` it to reach its types",
                ast.name_of(written_qualifier.expect("resolve only fails with a qualifier"))
            ),
            span,
        ));
    };
    let lookup = |defs: &[TypeDef]| -> Option<usize> {
        defs.iter().position(|d| d.name == written_name && d.visible_from(target))
    };

    // `Box[[T]]` is the one place an unsized referent may stand as a type
    // argument (`docs/boxed-slices.md` §2). Everywhere else `[T]` has no
    // size of its own and only a reference may point at one; a box is a
    // pointer *and* a length precisely so that it can hold one.
    let boxed = lookup(defs).is_some_and(|i| defs[i].def.0 as usize == PRELUDE_BOX);
    let args = written_args
        .iter()
        .map(|arg| resolve_type_at(cx, params, regions, *arg, boxed))
        .collect::<Result<Vec<_>, _>>()?;

    if let Some(index) = generics.iter().position(|g| *g == written_name) {
        if !args.is_empty() {
            return Err(Diagnostic::new(
                Rule::TypeArgsNotTaken,
                format!("type parameter `{name}` takes no type arguments"),
                span,
            ));
        }
        return Ok(Type::Param(index as u32));
    }

    let (ty, arity) = match name {
        "int" => (Type::Int, 0),
        "byte" => (Type::Byte, 0),
        "float" => (Type::Float, 0),
        "bool" => (Type::Bool, 0),
        other => match lookup(defs) {
            Some(index) => {
                let def = &defs[index];
                // §5: private is private, including in a type. A caller
                // that cannot name the type cannot use what mentions it.
                if target != module && !def.public {
                    return Err(Diagnostic::new(
                        Rule::NotPublic,
                        format!(
                            "`{other}` is not `pub`, so it cannot be named from another module"
                        ),
                        span,
                    ));
                }
                // Where a `val` bound on a type parameter is kept
                // (`docs/collections.md` §3, `mode-polymorphism.md` §2).
                //
                // Two bounds meet here. Declaring an aggregate `val` is a
                // promise about *every* instantiation, and it used to be
                // believed rather than checked -- a leak and a double free.
                // A `res` or undeclared aggregate promises nothing, so it
                // writes its bound instead, which is what lets a vector own
                // an allocation and still require copyable elements.
                //
                // The mode of an argument is read against **this**
                // declaration's bounds -- the one the type is written
                // inside. Reading it against nothing made a rigid `T` `res`
                // however it was bounded, so a `[T: val]` function could not
                // name a `val` aggregate at `T` at all (§4).
                for (i, argument) in args.iter().enumerate() {
                    if def.bound(i) != Some(Mode::Val) {
                        continue;
                    }
                    if mode_of(defs, unifier, params.bounds, argument) != Mode::Res {
                        continue;
                    }
                    // Named as the source wrote it: a type parameter renders
                    // as `T`, not as the `T0` the unifier falls back to when
                    // no declaration has set its parameter names.
                    let written = match argument {
                        Type::Param(i) => generics
                            .get(*i as usize)
                            .map(|g| ast.name_of(*g).to_owned())
                            .unwrap_or_else(|| unifier.display(argument)),
                        other => unifier.display(other),
                    };
                    let because = if def.declared_mode == Some(Mode::Val) {
                        format!("`{other}` is declared `val`, so its type arguments are `val` too")
                    } else {
                        format!(
                            "`{other}` bounds `{}` by `val`",
                            def.generics
                                .get(i)
                                .map(|g| ast.name_of(*g).to_owned())
                                .unwrap_or_else(|| i.to_string())
                        )
                    };
                    return Err(Diagnostic::new(
                        Rule::ModeBoundViolated,
                        format!("{because}, and `{written}` is `res`"),
                        span,
                    ));
                }
                (Type::Named(def.def, args.clone()), def.generics.len())
            }
            None => {
                return Err(Diagnostic::new(
                    Rule::UnknownName,
                    format!("unknown type `{other}`"),
                    span,
                ));
            }
        },
    };

    if args.len() != arity {
        return Err(Diagnostic::new(
            Rule::ArityMismatch,
            if arity == 0 {
                format!("`{name}` takes no type arguments")
            } else {
                format!(
                    "`{name}` takes {arity} type argument{}, but {} {} given",
                    if arity == 1 { "" } else { "s" },
                    args.len(),
                    if args.len() == 1 { "was" } else { "were" }
                )
            },
            span,
        ));
    }
    Ok(ty)
}
