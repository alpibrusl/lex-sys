//! Resolution, type checking, and lowering to the M1 intermediate
//! representation.
//!
//! One walk over each function body does all three. It is the only pass that
//! reads a body, and everything a *program* can be refused for is refused
//! here, so the backend receives IR that cannot fail.
//!
//! Why one walk rather than a checker followed by a lowering pass: both need
//! the same scope stack and the same resolution of every name, and running
//! that twice means two places to disagree about what a name means. The type
//! vocabulary lives in `lex-sys-types`; this crate drives it.
//!
//! `docs/linearity-and-effects.md` §13 names three things M1 has to get right
//! before M2 can exist, and they are load-bearing here:
//!
//! 1. **A signature is complete and is the unit of checking.** Every parameter
//!    and return type is written. A body is checked against other functions'
//!    signatures, never against their bodies, so nothing is inferred across a
//!    boundary.
//! 2. **Types compare cheaply and canonically** — that is `lex-sys-types`.
//! 3. **The branch join is a real operation.** [`terminates`] is that join for
//!    control flow today; M2 adds a live set to the same shape.

use std::fmt;

use lex_sys_syntax::ast::{
    self, Ast, Block, Expr as AstExpr, ExprId, Item, Stmt as AstStmt, StmtId, Symbol, TypeExpr,
    TypeId,
};
use lex_sys_syntax::rules::Rule;
use lex_sys_syntax::span::{Diagnostic, Span};
use lex_sys_types::{DefId, Region, Type, Unifier, UnifyError};

mod fold;
mod linear;

pub use linear::Mode;
use linear::{Event, Trace, mode_of};

mod builtin;
mod defs;
mod function;
mod ir;
mod lower;

pub use builtin::*;
pub use defs::*;
use function::*;
pub use ir::*;
use lower::*;

/// Resolve and check an AST, producing IR a backend can lower without failing.
pub fn lower(ast: &Ast) -> Result<Program, Diagnostic> {
    lower_all(ast).map_err(|mut all| all.remove(0))
}

/// Lower a program, answering **every** independent refusal rather than
/// the first (`docs/agent-errors.md` §4).
///
/// The rule that decides what is independent: checking is total and runs
/// per declaration (`standard-library.md` §5.2), so one function's body
/// failing does not make the next function's body unknowable. Those are
/// collected. Everything before pass 1 — imports, type collection,
/// signatures — stops at the first refusal, because a program whose
/// shape is not yet known has no reliable second error, and a checker
/// inventing one teaches its reader to chase phantoms.
///
/// The `Vec` is never empty on the error path, and its entries are in
/// source order, which is the order pass 1 already ran in.
pub fn lower_all(ast: &Ast) -> Result<Program, Vec<Diagnostic>> {
    // The first refusal comes back through `?` like any other, and pass
    // 1's remaining ones ride in `rest` — which keeps every `?` in the
    // body below returning one `Diagnostic`, as it always did, rather
    // than turning a 400-line function inside out to carry a `Vec`.
    let mut rest = Vec::new();
    match lower_inner(ast, &mut rest) {
        Ok(program) => Ok(program),
        Err(first) => {
            let mut all = vec![first];
            all.append(&mut rest);
            Err(all)
        }
    }
}

fn lower_inner(ast: &Ast, rest: &mut Vec<Diagnostic>) -> Result<Program, Diagnostic> {
    check_imports(ast)?;
    let mut unifier = Unifier::new();
    let defs = collect_types(ast, &mut unifier)?;

    // Every function is visible to every other, so collect signatures before
    // checking any body. Definition order in the file is irrelevant, and no
    // body is ever consulted to type a call.
    // §8.4: a foreign signature is written once, here, and every caller is
    // checked against it exactly as against a written function's.
    let mut externs: Vec<ExternFn> = Vec::new();
    for (index, item) in ast.items.iter().enumerate() {
        let Item::Extern(decl) = item else { continue };
        let name = ast.name_of(decl.name);
        let item_id = ast::ItemId(index as u32);
        // An `extern` has no `pub` and lives wherever it is written; a
        // foreign symbol is global to the linker either way, so the module
        // is only for resolving the name.
        let module = ast.module_of(item_id);
        let span = ast.item_span(item_id);
        if Builtin::from_name(name).is_some() {
            return Err(Diagnostic::new(
                Rule::ForeignDeclaration,
                format!("`{name}` is a builtin and cannot be declared foreign"),
                span,
            ));
        }
        // Twice in one module is a duplicate name; twice in two modules
        // would be two names for one linker symbol, which is also a
        // mistake -- so the *symbol* stays program-wide unique while the
        // name is module-scoped (`docs/modules.md` §3).
        if externs.iter().any(|e| e.name == name && e.module == module) {
            return Err(Diagnostic::new(
                Rule::DuplicateDeclaration,
                format!("foreign function `{name}` is declared twice"),
                span,
            ));
        }
        if let Some(clash) = externs.iter().find(|e| e.symbol == decl.symbol) {
            return Err(Diagnostic::new(
                Rule::ForeignDeclaration,
                format!(
                    "`{name}` binds the foreign symbol `{}`, which `{}` already binds; a symbol is one function to the linker",
                    decl.symbol, clash.name
                ),
                span,
            ));
        }
        let region_scope = check_region_names(ast, &decl.regions, &[], span)?;
        let params = decl
            .params
            .iter()
            .map(|p| {
                resolve_type(
                    Resolving { ast, defs: &defs, unifier: &unifier, module },
                    Params::unbounded(&[]),
                    &region_scope,
                    p.ty,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ret = resolve_type(
            Resolving { ast, defs: &defs, unifier: &unifier, module },
            Params::unbounded(&[]),
            &region_scope,
            decl.ret,
        )?;
        let declared =
            Effects::new(decl.effects.iter().map(|e| Label {
                name: ast.name_of(e.name).to_owned(),
                argument: e.argument.clone(),
            }));

        // §8.4: what crosses the boundary is what C can name. Aggregates
        // have no layout contract here yet, and a reference to anything but
        // a capability would be a pointer this compiler has not promised to
        // lay out — so both are refused at the declaration, where the author
        // can still write something else, rather than at the call.
        for (param, decl_param) in params.iter().zip(&decl.params) {
            let what = ast.name_of(decl_param.name);
            match param {
                Type::Int | Type::Bool => {}
                Type::Ref { inner, .. } if matches!(inner.as_ref(), Type::Named(def, _) if is_capability(*def)) =>
                    {}
                // `docs/strings.md` §6: a byte slice crosses as a pointer
                // *and* a separate length, because C has no notion of the
                // pair. That is what makes `write(fd, ptr, len)`
                // expressible and `strlen(ptr)` not: this design puts no
                // NUL anywhere, and the functions taking an explicit length
                // are the ones that cannot run off the end.
                Type::Ref { inner, .. } if matches!(inner.as_ref(), Type::Slice(element) if **element == Type::Byte) =>
                    {}
                Type::Ref { .. } => {
                    return Err(Diagnostic::new(
                        Rule::ForeignBoundaryType,
                        format!(
                            "`{name}` takes `{what}` by reference, and the only references that cross a foreign boundary are a borrowed capability and a `[byte]` slice: C is not told about regions"
                        ),
                        span,
                    ));
                }
                other => {
                    return Err(Diagnostic::new(
                        Rule::ForeignBoundaryType,
                        format!(
                            "`{name}` takes `{what}` of type `{}`, which has no agreed layout across a foreign boundary; a foreign parameter is `int`, `bool`, or a borrowed capability",
                            unifier.display(other)
                        ),
                        span,
                    ));
                }
            }
        }
        // `Type::Unit` is allowed here and cannot arrive here: `tuples.md`
        // §4 keeps it out of the source grammar deliberately, so no
        // declaration can name it. The arm stays because the *check* is
        // about layout and unit has one; the message does not mention it,
        // because a message that names a type the grammar refuses sends a
        // programmer to write `()` and be told there is no `()`
        // (`docs/reach.md` §4).
        if !matches!(ret, Type::Int | Type::Bool | Type::Unit) {
            return Err(Diagnostic::new(
                Rule::ForeignBoundaryType,
                format!(
                    "`{name}` returns `{}`, which has no agreed layout across a foreign boundary; a foreign result is `int` or `bool`, and a C function that returns nothing is declared `int` and its result discarded",
                    unifier.display(&ret)
                ),
                span,
            ));
        }

        // §8.4: the capability is the only way to reach the foreign call, so
        // the row and the capability parameters have to agree. A declaration
        // that named one library and borrowed another would be the one place
        // a foreign signature is written, written wrong.
        let mut authorised = Effects::pure();
        for param in &params {
            if let Type::Ref { inner, .. } = param {
                authorised.union(&discharged_by(&defs, inner));
            }
        }
        for param in &params {
            let Type::Ref { inner, .. } = param else { continue };
            if let Type::Named(def, args) = inner.as_ref()
                && def.0 as usize == PRELUDE_FFI
                && matches!(args.first(), Some(Type::Lit(library)) if library == FFI_ROOT)
            {
                return Err(Diagnostic::new(
                    Rule::ForeignDeclaration,
                    format!(
                        "`{name}` borrows the unnarrowed `Ffi(\"\")`, which names no library; a foreign declaration names the library it calls into, so narrow before declaring"
                    ),
                    span,
                ));
            }
        }
        if let Some(label) = declared.missing_from(&authorised) {
            return Err(Diagnostic::new(
                Rule::EffectNotDeclared,
                format!(
                    "`{name}` declares `{label}` but holds no capability that authorises it; a foreign call is reached through the capability that names its library"
                ),
                span,
            ));
        }
        if let Some(label) = authorised.missing_from(&declared) {
            return Err(Diagnostic::new(
                Rule::EffectNotDeclared,
                format!(
                    "`{name}` borrows a capability authorising `{label}`, which its row {declared} does not declare"
                ),
                span,
            ));
        }

        externs.push(ExternFn {
            name: name.to_owned(),
            module,
            symbol: decl.symbol.clone(),
            params,
            effects: declared,
            ret,
        });
    }

    // `docs/compile-time-data.md` §2. Collected before any body is
    // lowered, because a function may read a `static` declared below it —
    // the same reason signatures are collected before bodies.
    let mut statics: Vec<StaticDef> = Vec::new();
    let mut static_items: Vec<usize> = Vec::new();
    for (index, item) in ast.items.iter().enumerate() {
        let Item::Static(decl) = item else { continue };
        let item_id = ast::ItemId(index as u32);
        let module = ast.module_of(item_id);
        let span = ast.item_span(item_id);
        let name = ast.name_of(decl.name);
        if statics.iter().any(|d| d.name == decl.name && d.module == module) {
            return Err(Diagnostic::new(
                Rule::DuplicateDeclaration,
                format!("`static {name}` is declared twice"),
                span,
            ));
        }
        // The *referent*, so a bare `[int]` is what is written: a
        // `static` names what the data is, and the `&static` in front of
        // it is what every reader gets rather than what the author types
        // (`docs/compile-time-data.md` §2).
        let referent = resolve_type_at(
            Resolving { ast, defs: &defs, unifier: &unifier, module },
            Params { names: &[], bounds: &[] },
            &[],
            decl.ty,
            true,
        )?;
        // §2 and §6: the item is for *data*, and the evaluator's store
        // holds a run of scalars. A scalar `static` is what §3 of
        // `compile-time.md` already folds, so it would be a second way to
        // say one thing.
        let Type::Slice(element) = &referent else {
            return Err(Diagnostic::new(
                Rule::StaticItem,
                format!(
                    "`static {name}` must be a slice — `[int]`, `[byte]`, `[bool]` or `[float]`; a `static` is for data, and a scalar constant is already folded where it is written (`docs/compile-time.md` §3)"
                ),
                span,
            ));
        };
        if !matches!(**element, Type::Int | Type::Byte | Type::Bool | Type::Float) {
            return Err(Diagnostic::new(
                Rule::StaticItem,
                format!(
                    "`static {name}` holds `{}`, and a `static` holds scalars: `int`, `byte`, `bool` or `float` (`docs/compile-time-data.md` §6)",
                    unifier.display(element)
                ),
                span,
            ));
        }
        static_items.push(index);
        statics.push(StaticDef { name: decl.name, module, referent });
    }

    let mut signatures: Vec<Signature> = Vec::new();
    for (index, item) in ast.items.iter().enumerate() {
        let Item::Fn(decl) = item else { continue };
        let name = ast.name_of(decl.name);
        let item_id = ast::ItemId(index as u32);
        let module = ast.module_of(item_id);
        let span = ast.item_span(item_id);

        if Builtin::from_name(name).is_some() {
            return Err(Diagnostic::new(
                Rule::BuiltinRedeclared,
                format!("`{name}` is a builtin and cannot be redefined"),
                span,
            ));
        }
        // Scoped to the module, like a type declaration: two modules may
        // each define `print_nat` (`docs/modules.md` §3).
        if signatures.iter().any(|s| s.name == decl.name && s.module == module) {
            return Err(Diagnostic::new(
                Rule::DuplicateDeclaration,
                format!("function `{name}` is defined twice"),
                span,
            ));
        }
        // A foreign declaration and a written function are two answers to
        // the same call, and a call resolves to one thing -- within one
        // module. Across modules they are two names (`docs/modules.md`
        // §3), and only the `extern` binds a linker symbol.
        if externs.iter().any(|e| e.name == name && e.module == module) {
            return Err(Diagnostic::new(
                Rule::ForeignDeclaration,
                format!("`{name}` is already declared foreign, so this name is taken"),
                span,
            ));
        }
        check_generic_names(ast, &decl.generics, span)?;
        let region_scope = check_region_names(ast, &decl.regions, &decl.generics, span)?;

        // `where a <= b` names two region parameters, resolved to their
        // positions so the relation is integers from here on (§5.2).
        let mut outlives = Vec::new();
        for (inner, outer) in &decl.outlives {
            let position = |sym: &Symbol| decl.regions.iter().position(|r| r == sym);
            let (Some(a), Some(b)) = (position(inner), position(outer)) else {
                let missing = if position(inner).is_none() {
                    ast.name_of(*inner)
                } else {
                    ast.name_of(*outer)
                };
                return Err(Diagnostic::new(
                    Rule::RegionMismatch,
                    format!(
                        "`{missing}` is not a region parameter of `{name}`; a `where` clause relates the regions the declaration takes"
                    ),
                    span,
                ));
            };
            outlives.push((a as u32, b as u32));
        }

        let mut seen: Vec<Symbol> = Vec::new();
        let mut params = Vec::new();
        for param in &decl.params {
            if seen.contains(&param.name) {
                return Err(Diagnostic::new(
                    Rule::DuplicateDeclaration,
                    format!("parameter `{}` is bound twice", ast.name_of(param.name)),
                    span,
                ));
            }
            seen.push(param.name);
            params.push(resolve_type(
                Resolving { ast, defs: &defs, unifier: &unifier, module },
                Params { names: &decl.generics, bounds: &decl.bounds },
                &region_scope,
                param.ty,
            )?);
        }

        let ret = resolve_type(
            Resolving { ast, defs: &defs, unifier: &unifier, module },
            Params { names: &decl.generics, bounds: &decl.bounds },
            &region_scope,
            decl.ret,
        )?;
        // §5: a usable signature names usable types. A `pub fn` whose
        // parameter or return type is private to this module cannot be
        // called from outside it -- the caller has no way to name the
        // type -- so the `pub` is a promise the declaration cannot keep.
        if decl.public {
            for ty in params.iter().chain(std::iter::once(&ret)) {
                if let Some(private) = private_type_in(&defs, module, ty) {
                    return Err(Diagnostic::new(
                        Rule::NotPublic,
                        format!(
                            "`{name}` is `pub`, but its signature names `{}`, which is not; a caller in another module could not write the type",
                            ast.name_of(private)
                        ),
                        span,
                    ));
                }
            }
        }
        signatures.push(Signature {
            bounds: decl.bounds.clone(),
            module,
            public: decl.public,
            name: decl.name,
            generics: decl.generics.clone(),
            regions: decl.regions.clone(),
            outlives,
            effects: Effects::new(decl.effects.iter().map(|e| Label {
                name: ast.name_of(e.name).to_owned(),
                argument: e.argument.clone(),
            })),
            params,
            ret,
            item: index,
        });
    }

    // Pass 1: check **every** function, once.
    //
    // Checking is total and emission is not, which is the split that lets
    // `docs/standard-library.md` §5.2 be true: a library declaration
    // nobody calls is still type-checked, and still costs no bytes.
    //
    // A generic function is checked with its parameters *rigid*, which is
    // the stronger check as well as the only one available: a body that
    // type-checks for every `T` is checked once, rather than once per
    // instantiation and never for the `T` nobody used.
    //
    // Every refusal here is collected rather than returned
    // (`docs/agent-errors.md` §4): these bodies are independent of each
    // other, so stopping at the first costs a reader nothing and costs a
    // program one compile per error.
    let mut refused: Vec<Diagnostic> = Vec::new();
    for (index, signature) in signatures.iter().enumerate() {
        let rigid: Vec<Type> = (0..signature.generics.len() as u32).map(Type::Param).collect();
        let mut checking = Mono::new(false);
        if let Err(error) = lower_function(
            ast,
            &defs,
            &signatures,
            &statics,
            &externs,
            &mut unifier,
            index,
            &rigid,
            &mut checking,
        ) {
            refused.push(error);
        }
    }
    if !refused.is_empty() {
        let first = refused.remove(0);
        *rest = refused;
        return Err(first);
    }

    // Pass 2: emit a copy of every function actually reachable.
    //
    // The roots are `main` and nothing else -- a program is what its
    // entry point reaches. Before `docs/standard-library.md` this seeded
    // from every non-generic function instead, which was indistinguishable
    // while every function in a program was one somebody wrote; with a
    // standard library it is the difference between a 1 KB object and a
    // 7 KB one for a program that calls none of it.
    //
    // With no `main` there is no program, only declarations -- `lex-sys
    // check` on a library on its own, which has to check all of them. So
    // every non-generic function is a root in that case, which is the
    // binary-versus-library distinction every toolchain draws, drawn by
    // the one fact available here.
    let mut mono = Mono::new(true);
    let entry =
        signatures.iter().position(|s| ast.name_of(s.name) == "main" && s.generics.is_empty());
    match entry {
        Some(index) => {
            mono.request(index, Vec::new());
        }
        None => {
            for (index, signature) in signatures.iter().enumerate() {
                if signature.generics.is_empty() {
                    mono.request(index, Vec::new());
                }
            }
        }
    }

    // `docs/compile-time-data.md` §2. Lowered *before* the worklist is
    // drained, because a `static` body calls functions and those calls
    // request instances: a function only a `static` reaches is a root of
    // the program exactly as `main`'s callees are, and draining first
    // would leave it unlowered.
    let mut static_bodies: Vec<Func> = Vec::new();
    for (index, item) in static_items.iter().enumerate() {
        static_bodies.push(lower_static(
            ast,
            &defs,
            &signatures,
            &statics,
            &externs,
            &mut unifier,
            index as u32,
            *item,
            &mut mono,
        )?);
    }

    let mut funcs: Vec<Option<Func>> = Vec::new();
    while let Some(instance) = mono.pending.pop() {
        let (signature, args) =
            (mono.instances[instance].signature, mono.instances[instance].args.clone());
        let func = lower_function(
            ast,
            &defs,
            &signatures,
            &statics,
            &externs,
            &mut unifier,
            signature,
            &args,
            &mut mono,
        )?;
        if funcs.len() <= instance {
            funcs.resize_with(instance + 1, || None);
        }
        funcs[instance] = Some(func);
    }

    let mut program = Program {
        funcs: funcs.into_iter().map(|f| f.expect("every requested instance is lowered")).collect(),
        statics: Vec::new(),
        folded_calls: 0,
        folded_late: 0,
        externs: externs.clone(),
        types: defs
            .iter()
            .map(|d| match &d.kind {
                DefKind::Struct(fields) => TypeInfo::Struct {
                    name: ast.name_of(d.name).to_owned(),
                    fields: fields
                        .iter()
                        .map(|(n, t)| (ast.name_of(*n).to_owned(), t.clone()))
                        .collect(),
                },
                DefKind::Enum(variants) => TypeInfo::Enum {
                    name: ast.name_of(d.name).to_owned(),
                    variants: variants
                        .iter()
                        .map(|(n, p)| (ast.name_of(*n).to_owned(), p.clone()))
                        .collect(),
                },
            })
            .collect(),
    };

    // `docs/compile-time.md` §3. Operators were folded during lowering,
    // where a trap still had a span to point at (§4); this is the other
    // half, and it runs here rather than there because a call may name a
    // function declared further down the file.
    let tally = fold::evaluate_calls(&mut program);
    program.folded_calls = tally.calls;
    program.folded_late = tally.operators;

    // `docs/compile-time-data.md` §3. Last, because a `static` body may
    // call any pure function in the program and those have to be lowered
    // and folded first — and in declaration order, because a `static` may
    // read one declared before it.
    //
    // The bodies themselves were lowered before the worklist was drained,
    // above; this is only the running of them.
    let mut evaluated: Vec<Vec<i64>> = Vec::new();
    for (index, def) in statics.iter().enumerate() {
        let item = static_items[index];
        let body = &static_bodies[index];
        let name = ast.name_of(def.name).to_owned();
        let values = fold::evaluate_static(&program, body, &evaluated).map_err(|why| {
            Diagnostic::new(
                Rule::ConstantTraps,
                format!("`static {name}` cannot be evaluated: {why}"),
                ast.item_span(ast::ItemId(item as u32)),
            )
        })?;
        let Type::Slice(element) = &def.referent else {
            unreachable!("a static's referent is checked to be a slice when it is collected");
        };
        evaluated.push(values.clone());
        program.statics.push(StaticValue { name, element: (**element).clone(), values });
    }
    Ok(program)
}

#[cfg(test)]
#[path = "tests/unit.rs"]
mod tests;

/// Effect rows: `docs/linearity-and-effects.md` §7.
#[cfg(test)]
#[path = "tests/effect.rs"]
mod effect_tests;

/// Capabilities: `docs/linearity-and-effects.md` §8.
#[cfg(test)]
#[path = "tests/capability.rs"]
mod capability_tests;

/// Strings: `docs/strings.md`.
#[cfg(test)]
#[path = "tests/string.rs"]
mod string_tests;

/// Slices: M3's first half, built on §5's references.
#[cfg(test)]
#[path = "tests/slice.rs"]
mod slice_tests;

/// Defined behaviour: `docs/defined-behaviour.md`.
#[cfg(test)]
#[path = "tests/defined_behaviour.rs"]
mod defined_behaviour_tests;

/// Arenas: `docs/linearity-and-effects.md` §6.
#[cfg(test)]
#[path = "tests/arena.rs"]
mod arena_tests;

/// Narrowing and foreign calls: `docs/linearity-and-effects.md` §7.4 and §8.4.
#[cfg(test)]
#[path = "tests/foreign.rs"]
mod foreign_tests;

/// Modes and linearity: `docs/linearity-and-effects.md` §3 and §4.
#[cfg(test)]
#[path = "tests/linearity.rs"]
mod linearity_tests;
