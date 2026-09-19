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

use lex_sys_syntax::ast::{
    self, Ast, Block, Expr as AstExpr, ExprId, Item, Stmt as AstStmt, StmtId, Symbol, TypeId,
};
use lex_sys_syntax::span::{Diagnostic, Span};
use lex_sys_types::{DefId, Type, Unifier, UnifyError};

/// A local variable. Parameters occupy slots `0..n_params`; each `let`/`var`
/// takes the next slot and never reuses one, so a shadowing binding is simply
/// a different slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot(pub u32);

/// Index into [`Program::funcs`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuncId(pub u32);

/// Functions the compiler provides rather than the program defining them.
///
/// M0/M1 scaffolding: `putchar` is how a program produces output before there
/// is any FFI. M2 replaces it with a capability-gated foreign call — output is
/// an effect, and an effect must be granted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Builtin {
    /// `putchar(c: int) -> int` — libc's, byte for byte.
    PutChar,
}

impl Builtin {
    pub const ALL: &'static [Builtin] = &[Builtin::PutChar];

    pub fn name(self) -> &'static str {
        match self {
            Builtin::PutChar => "putchar",
        }
    }

    /// The libc symbol the backend calls.
    pub fn symbol(self) -> &'static str {
        match self {
            Builtin::PutChar => "putchar",
        }
    }

    pub fn signature(self) -> (Vec<Type>, Type) {
        match self {
            Builtin::PutChar => (vec![Type::Int], Type::Int),
        }
    }

    pub fn from_name(name: &str) -> Option<Builtin> {
        Builtin::ALL.iter().copied().find(|b| b.name() == name)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    /// Truncating signed division. Division by zero and `int::MIN / -1` trap;
    /// neither is undefined behaviour.
    Div,
    /// Remainder, with the sign of the dividend. Traps on the same two inputs
    /// as `Div`.
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// `&&` and `||` short-circuit, so the backend lowers them as control flow
    /// rather than as an instruction.
    And,
    Or,
}

impl BinOp {
    pub fn is_comparison(self) -> bool {
        matches!(self, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge)
    }

    pub fn is_short_circuit(self) -> bool {
        matches!(self, BinOp::And | BinOp::Or)
    }

    /// The type this operator produces, given the type of its operands.
    pub fn result(self, operand: &Type) -> Type {
        if self.is_comparison() || self.is_short_circuit() { Type::Bool } else { operand.clone() }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Callee {
    Fn(FuncId),
    Builtin(Builtin),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Expr {
    Int(i64),
    Bool(bool),
    Load(Slot),
    /// A struct value. Fields are in *declaration* order whatever order they
    /// were written in, so the backend never has to consult a name.
    Struct {
        def: DefId,
        fields: Vec<Expr>,
    },
    /// `base.index`, by declaration position rather than by name. The struct
    /// is named too, so the backend never has to re-derive the base's type.
    Field {
        base: Box<Expr>,
        def: DefId,
        index: u32,
    },
    /// An enum value: which variant, and its payload.
    Enum {
        def: DefId,
        variant: u32,
        payload: Vec<Expr>,
    },
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Call {
        callee: Callee,
        args: Vec<Expr>,
    },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Stmt {
    /// Both `let`/`var` and plain assignment: by this point the difference is
    /// spent, having been checked during lowering.
    Store {
        slot: Slot,
        value: Expr,
    },
    /// An expression evaluated for its effects; its value is discarded.
    Eval(Expr),
    If {
        cond: Expr,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    Match {
        scrutinee: Expr,
        def: DefId,
        arms: Vec<Arm>,
    },
    Return(Expr),
}

/// One arm of a `match`, with its pattern already resolved to a variant index
/// and its bindings to slots.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Arm {
    /// `None` is the wildcard arm.
    pub variant: Option<u32>,
    /// One per payload position; `None` where the pattern wrote `_`.
    pub bindings: Vec<Option<Slot>>,
    pub body: Vec<Stmt>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Func {
    pub name: String,
    pub n_params: u32,
    /// One entry per slot, parameters first. The backend reads these to pick a
    /// machine type, so every slot's type is resolved before it gets here.
    pub slots: Vec<Type>,
    pub ret: Type,
    pub body: Vec<Stmt>,
}

impl Func {
    pub fn n_slots(&self) -> u32 {
        self.slots.len() as u32
    }
}

/// A declared type, as the backend needs it: names for diagnostics and member
/// types in declaration order.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TypeInfo {
    Struct { name: String, fields: Vec<(String, Type)> },
    Enum { name: String, variants: Vec<(String, Vec<Type>)> },
}

impl TypeInfo {
    pub fn name(&self) -> &str {
        match self {
            TypeInfo::Struct { name, .. } | TypeInfo::Enum { name, .. } => name,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Program {
    pub funcs: Vec<Func>,
    /// Indexed by [`DefId`].
    pub types: Vec<TypeInfo>,
}

impl Program {
    pub fn func(&self, id: FuncId) -> &Func {
        &self.funcs[id.0 as usize]
    }

    pub fn type_info(&self, def: DefId) -> &TypeInfo {
        &self.types[def.0 as usize]
    }

    pub fn find(&self, name: &str) -> Option<FuncId> {
        self.funcs.iter().position(|f| f.name == name).map(|i| FuncId(i as u32))
    }
}

/// Does every path through these statements end in a `return`?
///
/// Deliberately structural and therefore conservative: a `while` never counts,
/// even `while true { }`. A conservative answer is a total one, and a program
/// can always say `return` once more.
///
/// The backend asks the same question, so the two agree by construction.
pub fn terminates(body: &[Stmt]) -> bool {
    match body.last() {
        Some(Stmt::Return(_)) => true,
        Some(Stmt::If { then_body, else_body, .. }) => {
            !else_body.is_empty() && terminates(then_body) && terminates(else_body)
        }
        // A `match` reaching here is exhaustive -- the checker refuses any
        // other kind -- so if every arm returns, so does the match.
        Some(Stmt::Match { arms, .. }) => arms.iter().all(|arm| terminates(&arm.body)),
        _ => false,
    }
}

/// A function's signature: what a caller is checked against, and all a caller
/// is ever checked against.
struct Signature {
    name: Symbol,
    params: Vec<Type>,
    ret: Type,
}

/// A declared type, as the checker needs it: interned names, so a member
/// lookup is an integer comparison.
enum DefKind {
    Struct(Vec<(Symbol, Type)>),
    Enum(Vec<(Symbol, Vec<Type>)>),
}

struct TypeDef {
    name: Symbol,
    def: DefId,
    kind: DefKind,
    span: Span,
}

impl TypeDef {
    /// Every type this one holds directly, for the size check.
    fn members(&self) -> Box<dyn Iterator<Item = &Type> + '_> {
        match &self.kind {
            DefKind::Struct(fields) => Box::new(fields.iter().map(|(_, ty)| ty)),
            DefKind::Enum(variants) => Box::new(variants.iter().flat_map(|(_, p)| p.iter())),
        }
    }
}

/// Can `from` reach `target` by following member types?
///
/// M1 has no references, so a type that contains itself — directly or through
/// others — has no finite size. There is no representation to pick and no
/// depth to stop at, so it is refused rather than approximated. That covers
/// `enum List { Nil, Cons(int, List) }` as much as a self-referential struct:
/// the classic linked list needs an indirection the language does not have
/// yet.
fn reaches(defs: &[TypeDef], from: usize, target: usize, seen: &mut [bool]) -> bool {
    if seen[from] {
        return false;
    }
    seen[from] = true;
    defs[from].members().any(|ty| match ty {
        Type::Named(def, _) => {
            let next = def.0 as usize;
            next == target || reaches(defs, next, target, seen)
        }
        _ => false,
    })
}

/// Collect every type declaration, in three passes.
///
/// Names first, so a member may mention a type declared later in the file;
/// then member types, which need those names; then the size check, which needs
/// every member type. Each pass needs the previous one complete, which is why
/// they are passes and not one loop.
fn collect_types(ast: &Ast, unifier: &mut Unifier) -> Result<Vec<TypeDef>, Diagnostic> {
    let mut defs: Vec<TypeDef> = Vec::new();

    for (index, item) in ast.items.iter().enumerate() {
        let (name_sym, noun) = match item {
            Item::Struct(decl) => (decl.name, "struct"),
            Item::Enum(decl) => (decl.name, "enum"),
            Item::Fn(_) => continue,
        };
        let span = ast.item_span(ast::ItemId(index as u32));
        let name = ast.name_of(name_sym);

        if matches!(name, "int" | "bool") {
            return Err(Diagnostic::new(
                format!("`{name}` is a built-in type and cannot be redeclared"),
                span,
            ));
        }
        if let Some(previous) = defs.iter().find(|d| d.name == name_sym) {
            let _ = previous;
            return Err(Diagnostic::new(format!("type `{name}` is declared twice"), span));
        }

        // Ids are handed out in declaration order, so `DefId(i)` indexes
        // `defs[i]` and the backend can use the same numbering.
        let def = unifier.declare(name);
        let kind = match noun {
            "struct" => DefKind::Struct(Vec::new()),
            _ => DefKind::Enum(Vec::new()),
        };
        defs.push(TypeDef { name: name_sym, def, kind, span });
    }

    for item in ast.items.iter() {
        match item {
            Item::Struct(decl) => {
                let position = defs.iter().position(|d| d.name == decl.name).expect("declared");
                let span = defs[position].span;
                let mut fields: Vec<(Symbol, Type)> = Vec::new();
                for field in &decl.fields {
                    if fields.iter().any(|(n, _)| *n == field.name) {
                        return Err(Diagnostic::new(
                            format!(
                                "field `{}` is declared twice in `{}`",
                                ast.name_of(field.name),
                                ast.name_of(decl.name)
                            ),
                            span,
                        ));
                    }
                    fields.push((field.name, resolve_type(ast, &defs, field.ty)?));
                }
                defs[position].kind = DefKind::Struct(fields);
            }
            Item::Enum(decl) => {
                let position = defs.iter().position(|d| d.name == decl.name).expect("declared");
                let span = defs[position].span;
                if decl.variants.is_empty() {
                    return Err(Diagnostic::new(
                        format!(
                            "enum `{}` has no variants, so no value of it can ever exist",
                            ast.name_of(decl.name)
                        ),
                        span,
                    ));
                }
                let mut variants: Vec<(Symbol, Vec<Type>)> = Vec::new();
                for variant in &decl.variants {
                    if variants.iter().any(|(n, _)| *n == variant.name) {
                        return Err(Diagnostic::new(
                            format!(
                                "variant `{}` is declared twice in `{}`",
                                ast.name_of(variant.name),
                                ast.name_of(decl.name)
                            ),
                            span,
                        ));
                    }
                    let payload = variant
                        .payload
                        .iter()
                        .map(|ty| resolve_type(ast, &defs, *ty))
                        .collect::<Result<Vec<_>, _>>()?;
                    variants.push((variant.name, payload));
                }
                defs[position].kind = DefKind::Enum(variants);
            }
            Item::Fn(_) => {}
        }
    }

    for index in 0..defs.len() {
        let mut seen = vec![false; defs.len()];
        if reaches(&defs, index, index, &mut seen) {
            return Err(Diagnostic::new(
                format!(
                    "type `{}` contains itself, so it has no finite size (M1 has no references)",
                    ast.name_of(defs[index].name)
                ),
                defs[index].span,
            ));
        }
    }

    Ok(defs)
}

/// Resolve and check an AST, producing IR a backend can lower without failing.
pub fn lower(ast: &Ast) -> Result<Program, Diagnostic> {
    let mut unifier = Unifier::new();
    let defs = collect_types(ast, &mut unifier)?;

    // Pass 1: every function is visible to every other, so collect signatures
    // before checking any body. Definition order in the file is irrelevant,
    // and no body is ever consulted to type a call.
    let mut signatures: Vec<Signature> = Vec::new();
    for (index, item) in ast.items.iter().enumerate() {
        let Item::Fn(decl) = item else { continue };
        let name = ast.name_of(decl.name);
        let span = ast.item_span(ast::ItemId(index as u32));

        if Builtin::from_name(name).is_some() {
            return Err(Diagnostic::new(
                format!("`{name}` is a builtin and cannot be redefined"),
                span,
            ));
        }
        if signatures.iter().any(|s| s.name == decl.name) {
            return Err(Diagnostic::new(format!("function `{name}` is defined twice"), span));
        }

        let mut seen: Vec<Symbol> = Vec::new();
        let mut params = Vec::new();
        for param in &decl.params {
            if seen.contains(&param.name) {
                return Err(Diagnostic::new(
                    format!("parameter `{}` is bound twice", ast.name_of(param.name)),
                    span,
                ));
            }
            seen.push(param.name);
            params.push(resolve_type(ast, &defs, param.ty)?);
        }

        let ret = resolve_type(ast, &defs, decl.ret)?;
        signatures.push(Signature { name: decl.name, params, ret });
    }

    let mut program = Program {
        funcs: Vec::new(),
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

    for (index, item) in ast.items.iter().enumerate() {
        let Item::Fn(decl) = item else { continue };
        let signature =
            signatures.iter().find(|s| s.name == decl.name).expect("collected in the pass above");

        let mut f = FnLowering {
            ast,
            signatures: &signatures,
            defs: &defs,
            unifier: &mut unifier,
            scopes: vec![Vec::new()],
            slots: Vec::new(),
            ret: signature.ret.clone(),
        };

        for (param, ty) in decl.params.iter().zip(signature.params.iter()) {
            // Parameters are immutable: the shape of a binding handed to you,
            // not one you own outright.
            f.declare(param.name, ty.clone(), false);
        }
        let body = f.block(&decl.body)?;
        let slots = f.slots.clone();

        if !terminates(&body) {
            return Err(Diagnostic::new(
                format!(
                    "function `{}` can finish without returning a value",
                    ast.name_of(decl.name)
                ),
                ast.item_span(ast::ItemId(index as u32)),
            ));
        }

        program.funcs.push(Func {
            name: ast.name_of(decl.name).to_owned(),
            n_params: decl.params.len() as u32,
            slots,
            ret: signature.ret.clone(),
            body,
        });
    }

    Ok(program)
}

/// Turn a written type into a real one.
///
/// M1 has two primitive types and no declared ones yet, so this is short. It
/// is a function rather than a match at each use site because unknown-type
/// errors must read the same wherever a type is written.
fn resolve_type(ast: &Ast, defs: &[TypeDef], id: TypeId) -> Result<Type, Diagnostic> {
    let written = ast.ty(id);
    let name = ast.name_of(written.name);
    let span = ast.type_span(id);

    let ty = match name {
        "int" => Type::Int,
        "bool" => Type::Bool,
        other => match defs.iter().find(|d| d.name == written.name) {
            Some(def) => Type::Named(def.def, Vec::new()),
            None => {
                return Err(Diagnostic::new(format!("unknown type `{other}`"), span));
            }
        },
    };

    // Nothing in M1 is generic yet, so every type takes zero arguments. The
    // written form already allows them, which is why this is a check rather
    // than a parse error.
    if !written.args.is_empty() {
        return Err(Diagnostic::new(format!("`{name}` takes no type arguments"), span));
    }
    Ok(ty)
}

struct Binding {
    name: Symbol,
    slot: Slot,
    ty: Type,
    mutable: bool,
}

struct FnLowering<'a> {
    ast: &'a Ast,
    signatures: &'a [Signature],
    defs: &'a [TypeDef],
    unifier: &'a mut Unifier,
    scopes: Vec<Vec<Binding>>,
    slots: Vec<Type>,
    ret: Type,
}

impl<'a> FnLowering<'a> {
    fn declare(&mut self, name: Symbol, ty: Type, mutable: bool) -> Slot {
        let slot = Slot(self.slots.len() as u32);
        self.slots.push(ty.clone());
        self.scopes.last_mut().expect("a scope is always open").push(Binding {
            name,
            slot,
            ty,
            mutable,
        });
        slot
    }

    fn lookup(&self, name: Symbol) -> Option<&Binding> {
        self.scopes.iter().rev().find_map(|scope| scope.iter().rev().find(|b| b.name == name))
    }

    fn declared_in_current_scope(&self, name: Symbol) -> bool {
        self.scopes.last().is_some_and(|scope| scope.iter().any(|b| b.name == name))
    }

    /// Require two types to be equal, reporting the failure at `span`.
    fn expect_type(&mut self, expected: &Type, found: &Type, span: Span) -> Result<(), Diagnostic> {
        match self.unifier.unify(expected, found) {
            Ok(()) => Ok(()),
            Err(UnifyError::Mismatch { expected, found }) => Err(Diagnostic::new(
                format!(
                    "expected `{}`, found `{}`",
                    self.unifier.display(&expected),
                    self.unifier.display(&found)
                ),
                span,
            )),
            Err(UnifyError::Infinite { ty, .. }) => Err(Diagnostic::new(
                format!("this would build an infinite type, `{}`", self.unifier.display(&ty)),
                span,
            )),
        }
    }

    fn block(&mut self, block: &Block) -> Result<Vec<Stmt>, Diagnostic> {
        self.scopes.push(Vec::new());
        let out = self.stmts(&block.stmts);
        self.scopes.pop();
        out
    }

    fn stmts(&mut self, ids: &[StmtId]) -> Result<Vec<Stmt>, Diagnostic> {
        let mut out: Vec<Stmt> = Vec::new();
        for (i, &id) in ids.iter().enumerate() {
            if i > 0 && terminates(&out) {
                return Err(Diagnostic::new(
                    "this statement is unreachable",
                    self.ast.stmt_span(id),
                ));
            }
            out.push(self.stmt(id)?);
        }
        Ok(out)
    }

    fn stmt(&mut self, id: StmtId) -> Result<Stmt, Diagnostic> {
        let span = self.ast.stmt_span(id);
        Ok(match self.ast.stmt(id) {
            AstStmt::Let { name, mutable, ty, value } => {
                // The initialiser is resolved *before* the binding exists, so
                // `let x = x;` reads the outer `x` or fails, and never itself.
                let (value, found) = self.expr(*value)?;
                let declared = match ty {
                    Some(written) => {
                        let declared = resolve_type(self.ast, self.defs, *written)?;
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
                if self.declared_in_current_scope(*name) {
                    return Err(Diagnostic::new(
                        format!(
                            "`{}` is already bound in this block (shadowing is only allowed in an inner block)",
                            self.ast.name_of(*name)
                        ),
                        span,
                    ));
                }
                let slot = self.declare(*name, declared, *mutable);
                Stmt::Store { slot, value }
            }
            AstStmt::Assign { name, value } => {
                let value_span = self.ast.expr_span(match self.ast.stmt(id) {
                    AstStmt::Assign { value, .. } => *value,
                    _ => unreachable!(),
                });
                let (value, found) = self.expr(*value)?;
                let text = self.ast.name_of(*name);
                let Some(binding) = self.lookup(*name) else {
                    return Err(Diagnostic::new(format!("`{text}` is not bound here"), span));
                };
                if !binding.mutable {
                    return Err(Diagnostic::new(
                        format!("`{text}` is immutable; declare it with `var` to assign to it"),
                        span,
                    ));
                }
                let (slot, declared) = (binding.slot, binding.ty.clone());
                self.expect_type(&declared, &found, value_span)?;
                Stmt::Store { slot, value }
            }
            AstStmt::Expr(e) => Stmt::Eval(self.expr(*e)?.0),
            AstStmt::If { cond, then_block, else_block } => {
                let cond = self.condition(*cond)?;
                let then_body = self.block(then_block)?;
                let else_body = match else_block {
                    Some(block) => self.block(block)?,
                    None => Vec::new(),
                };
                Stmt::If { cond, then_body, else_body }
            }
            AstStmt::While { cond, body } => {
                let cond = self.condition(*cond)?;
                let body = self.block(body)?;
                Stmt::While { cond, body }
            }
            AstStmt::Match { scrutinee, arms } => self.match_stmt(*scrutinee, arms, span)?,
            AstStmt::Return(e) => {
                let (value, found) = self.expr(*e)?;
                let ret = self.ret.clone();
                self.expect_type(&ret, &found, self.ast.expr_span(*e))?;
                Stmt::Return(value)
            }
        })
    }

    /// Check a `match`: the scrutinee is an enum, every arm names a variant of
    /// it, no variant is matched twice, and between them the arms cover
    /// everything.
    ///
    /// Exhaustiveness is the point. A `match` that silently did nothing for an
    /// unlisted variant would be a hole in the type system exactly where the
    /// type system is supposed to pay for itself.
    fn match_stmt(
        &mut self,
        scrutinee: ExprId,
        arms: &[ast::MatchArm],
        span: Span,
    ) -> Result<Stmt, Diagnostic> {
        let scrutinee_span = self.ast.expr_span(scrutinee);
        let (value, scrutinee_ty) = self.expr(scrutinee)?;
        let resolved = self.unifier.resolve(&scrutinee_ty);

        let Type::Named(def_id, _) = resolved else {
            return Err(Diagnostic::new(
                format!(
                    "`{}` cannot be matched (M1 matches enums)",
                    self.unifier.display(&resolved)
                ),
                scrutinee_span,
            ));
        };
        let def = self.defs.iter().find(|d| d.def == def_id).expect("a declared type");
        let enum_name = self.ast.name_of(def.name).to_owned();
        let DefKind::Enum(variants) = &def.kind else {
            return Err(Diagnostic::new(
                format!("`{enum_name}` is a struct, not an enum; there is nothing to match on"),
                scrutinee_span,
            ));
        };
        let variants = variants.clone();

        let mut covered = vec![false; variants.len()];
        let mut wildcard = false;
        let mut lowered: Vec<Arm> = Vec::new();

        for arm in arms {
            if wildcard {
                return Err(Diagnostic::new(
                    "this arm is unreachable: `_` above it already matches everything",
                    span,
                ));
            }

            let (variant_index, bindings) = match &arm.pattern {
                ast::Pattern::Wildcard => {
                    if covered.iter().all(|c| *c) {
                        return Err(Diagnostic::new(
                            format!(
                                "this `_` is unreachable: every variant of `{enum_name}` is already matched"
                            ),
                            span,
                        ));
                    }
                    wildcard = true;
                    (None, Vec::new())
                }
                ast::Pattern::Variant { enum_name: written, variant, bindings } => {
                    let written_text = self.ast.name_of(*written);
                    if *written != def.name {
                        return Err(Diagnostic::new(
                            format!(
                                "expected a variant of `{enum_name}`, found one of `{written_text}`"
                            ),
                            span,
                        ));
                    }
                    let variant_text = self.ast.name_of(*variant);
                    let Some(index) = variants.iter().position(|(n, _)| n == variant) else {
                        return Err(Diagnostic::new(
                            format!("`{enum_name}` has no variant `{variant_text}`"),
                            span,
                        ));
                    };
                    if covered[index] {
                        return Err(Diagnostic::new(
                            format!("`{enum_name}::{variant_text}` is matched twice"),
                            span,
                        ));
                    }
                    let payload = &variants[index].1;
                    if bindings.len() != payload.len() {
                        return Err(Diagnostic::new(
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
            let mut slots: Vec<Option<Slot>> = Vec::new();
            if let Some(index) = variant_index {
                let payload = variants[index as usize].1.clone();
                for (binding, ty) in bindings.iter().zip(payload.into_iter()) {
                    match binding {
                        Some(name) => {
                            if self.declared_in_current_scope(*name) {
                                self.scopes.pop();
                                return Err(Diagnostic::new(
                                    format!(
                                        "`{}` is bound twice in this pattern",
                                        self.ast.name_of(*name)
                                    ),
                                    span,
                                ));
                            }
                            slots.push(Some(self.declare(*name, ty, false)));
                        }
                        // `_` still occupies a payload position; it just has
                        // no name, so the backend drops the value.
                        None => slots.push(None),
                    }
                }
            }
            let body = self.stmts(&arm.body.stmts);
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
                format!("this `match` does not cover {}", missing.join(", ")),
                span,
            ));
        }

        Ok(Stmt::Match { scrutinee: value, def: def_id, arms: lowered })
    }

    /// A condition is a `bool`. M0 tested "non-zero"; M1 has a type for the
    /// question, so the convention becomes a rule.
    fn condition(&mut self, id: ExprId) -> Result<Expr, Diagnostic> {
        let (expr, found) = self.expr(id)?;
        self.expect_type(&Type::Bool, &found, self.ast.expr_span(id))?;
        Ok(expr)
    }

    fn expr(&mut self, id: ExprId) -> Result<(Expr, Type), Diagnostic> {
        let span = self.ast.expr_span(id);
        Ok(match self.ast.expr(id) {
            AstExpr::Int(v) => (Expr::Int(*v), Type::Int),
            AstExpr::Bool(v) => (Expr::Bool(*v), Type::Bool),
            AstExpr::Name(name) => {
                let text = self.ast.name_of(*name);
                match self.lookup(*name) {
                    Some(binding) => (Expr::Load(binding.slot), binding.ty.clone()),
                    None if self.signatures.iter().any(|s| s.name == *name)
                        || Builtin::from_name(text).is_some() =>
                    {
                        return Err(Diagnostic::new(
                            format!(
                                "`{text}` is a function; M1 has no function values, so it can only be called"
                            ),
                            span,
                        ));
                    }
                    None => {
                        return Err(Diagnostic::new(format!("`{text}` is not bound here"), span));
                    }
                }
            }
            AstExpr::StructLit { name, fields } => {
                let text = self.ast.name_of(*name);
                let Some(def) = self.defs.iter().find(|d| d.name == *name) else {
                    return Err(Diagnostic::new(format!("`{text}` is not a struct"), span));
                };
                let DefKind::Struct(fields_decl) = &def.kind else {
                    return Err(Diagnostic::new(
                        format!("`{text}` is an enum, not a struct"),
                        span,
                    ));
                };
                // Copied out before checking any field value, because
                // checking borrows `self` and the table lives beside it.
                let (def_id, declared): (DefId, Vec<(Symbol, Type)>) =
                    (def.def, fields_decl.clone());

                let mut values: Vec<Option<Expr>> = vec![None; declared.len()];
                for (field, value) in fields {
                    let field_text = self.ast.name_of(*field);
                    let Some(index) = declared.iter().position(|(n, _)| n == field) else {
                        return Err(Diagnostic::new(
                            format!("`{text}` has no field `{field_text}`"),
                            span,
                        ));
                    };
                    if values[index].is_some() {
                        return Err(Diagnostic::new(
                            format!("field `{field_text}` is given twice"),
                            span,
                        ));
                    }
                    let value_span = self.ast.expr_span(*value);
                    let (lowered, found) = self.expr(*value)?;
                    self.expect_type(&declared[index].1, &found, value_span)?;
                    values[index] = Some(lowered);
                }

                // A struct value has every field or it is not one. There is no
                // default to fall back on and no zero to invent.
                if let Some(missing) = values.iter().position(Option::is_none) {
                    return Err(Diagnostic::new(
                        format!(
                            "missing field `{}` in `{text}`",
                            self.ast.name_of(declared[missing].0)
                        ),
                        span,
                    ));
                }

                (
                    Expr::Struct {
                        def: def_id,
                        fields: values.into_iter().map(|v| v.expect("checked above")).collect(),
                    },
                    Type::Named(def_id, Vec::new()),
                )
            }
            AstExpr::Field { base, name } => {
                let base_span = self.ast.expr_span(*base);
                let (lowered, base_ty) = self.expr(*base)?;
                let resolved = self.unifier.resolve(&base_ty);
                let Type::Named(def_id, _) = resolved else {
                    return Err(Diagnostic::new(
                        format!("`{}` has no fields", self.unifier.display(&resolved)),
                        base_span,
                    ));
                };
                let def = self
                    .defs
                    .iter()
                    .find(|d| d.def == def_id)
                    .expect("a named type is a declared type");
                let field_text = self.ast.name_of(*name);
                let DefKind::Struct(fields) = &def.kind else {
                    return Err(Diagnostic::new(
                        format!(
                            "`{}` is an enum; its payload is read by matching on it, not with `.`",
                            self.ast.name_of(def.name)
                        ),
                        base_span,
                    ));
                };
                let Some(index) = fields.iter().position(|(n, _)| n == name) else {
                    return Err(Diagnostic::new(
                        format!("`{}` has no field `{field_text}`", self.ast.name_of(def.name)),
                        span,
                    ));
                };
                let ty = fields[index].1.clone();
                (Expr::Field { base: Box::new(lowered), def: def_id, index: index as u32 }, ty)
            }
            AstExpr::Variant { enum_name, variant, args } => {
                let enum_text = self.ast.name_of(*enum_name);
                let variant_text = self.ast.name_of(*variant);
                let Some(def) = self.defs.iter().find(|d| d.name == *enum_name) else {
                    return Err(Diagnostic::new(format!("`{enum_text}` is not an enum"), span));
                };
                let DefKind::Enum(variants) = &def.kind else {
                    return Err(Diagnostic::new(
                        format!("`{enum_text}` is a struct, not an enum"),
                        span,
                    ));
                };
                let Some(index) = variants.iter().position(|(n, _)| n == variant) else {
                    return Err(Diagnostic::new(
                        format!("`{enum_text}` has no variant `{variant_text}`"),
                        span,
                    ));
                };
                let (def_id, payload_types) = (def.def, variants[index].1.clone());

                if args.len() != payload_types.len() {
                    return Err(Diagnostic::new(
                        format!(
                            "`{enum_text}::{variant_text}` carries {} value{}, but {} {} given",
                            payload_types.len(),
                            if payload_types.len() == 1 { "" } else { "s" },
                            args.len(),
                            if args.len() == 1 { "was" } else { "were" }
                        ),
                        span,
                    ));
                }

                let mut payload = Vec::with_capacity(args.len());
                for (&arg, expected) in args.iter().zip(payload_types.iter()) {
                    let arg_span = self.ast.expr_span(arg);
                    let (value, found) = self.expr(arg)?;
                    self.expect_type(expected, &found, arg_span)?;
                    payload.push(value);
                }

                (
                    Expr::Enum { def: def_id, variant: index as u32, payload },
                    Type::Named(def_id, Vec::new()),
                )
            }
            AstExpr::Unary { op, operand } => {
                let operand_span = self.ast.expr_span(*operand);
                let (inner, found) = self.expr(*operand)?;
                match op {
                    ast::UnOp::Neg => {
                        self.expect_type(&Type::Int, &found, operand_span)?;
                        (Expr::Neg(Box::new(inner)), Type::Int)
                    }
                    ast::UnOp::Not => {
                        self.expect_type(&Type::Bool, &found, operand_span)?;
                        (Expr::Not(Box::new(inner)), Type::Bool)
                    }
                }
            }
            AstExpr::Binary { op, lhs, rhs } => {
                let op = bin_op(*op);
                let lhs_span = self.ast.expr_span(*lhs);
                let rhs_span = self.ast.expr_span(*rhs);
                let (l, lt) = self.expr(*lhs)?;
                let (r, rt) = self.expr(*rhs)?;

                // Both sides agree first, then the operator says what it
                // accepts. Reporting in that order blames the operand that
                // disagrees rather than the operator.
                self.expect_type(&lt, &rt, rhs_span)?;
                let operand = self.unifier.resolve(&lt);
                match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                        self.expect_type(&Type::Int, &operand, lhs_span)?;
                    }
                    BinOp::And | BinOp::Or => {
                        self.expect_type(&Type::Bool, &operand, lhs_span)?;
                    }
                    BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        self.expect_type(&Type::Int, &operand, lhs_span)?;
                    }
                    // `==` and `!=` compare two values of the same *scalar*
                    // type. Structs would need a field-wise comparison, which
                    // is a decision about what equality means rather than a
                    // missing instruction, so M1 refuses instead of guessing.
                    BinOp::Eq | BinOp::Ne => {
                        if !matches!(operand, Type::Int | Type::Bool) {
                            return Err(Diagnostic::new(
                                format!(
                                    "`{}` cannot be compared with `==` (M1 compares `int` and `bool`)",
                                    self.unifier.display(&operand)
                                ),
                                lhs_span,
                            ));
                        }
                    }
                }
                let result = op.result(&operand);
                (Expr::Bin { op, lhs: Box::new(l), rhs: Box::new(r) }, result)
            }
            AstExpr::Call { callee, args } => {
                let text = self.ast.name_of(*callee);
                if self.lookup(*callee).is_some() {
                    return Err(Diagnostic::new(
                        format!("`{text}` is a local binding, not a function"),
                        span,
                    ));
                }

                let (callee_ref, params, ret) = if let Some(builtin) = Builtin::from_name(text) {
                    let (params, ret) = builtin.signature();
                    (Callee::Builtin(builtin), params, ret)
                } else {
                    let index = self.signatures.iter().position(|s| s.name == *callee).ok_or_else(
                        || {
                            Diagnostic::new(
                                format!("`{text}` is not a function in this unit"),
                                span,
                            )
                        },
                    )?;
                    let signature = &self.signatures[index];
                    (
                        Callee::Fn(FuncId(index as u32)),
                        signature.params.clone(),
                        signature.ret.clone(),
                    )
                };

                if args.len() != params.len() {
                    return Err(Diagnostic::new(
                        format!(
                            "`{text}` takes {} argument{}, but {} {} given",
                            params.len(),
                            if params.len() == 1 { "" } else { "s" },
                            args.len(),
                            if args.len() == 1 { "was" } else { "were" }
                        ),
                        span,
                    ));
                }

                let mut lowered = Vec::with_capacity(args.len());
                for (&arg, expected) in args.iter().zip(params.iter()) {
                    let arg_span = self.ast.expr_span(arg);
                    let (value, found) = self.expr(arg)?;
                    self.expect_type(expected, &found, arg_span)?;
                    lowered.push(value);
                }
                (Expr::Call { callee: callee_ref, args: lowered }, ret)
            }
        })
    }
}

fn bin_op(op: ast::BinOp) -> BinOp {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_sys_syntax::parse;

    fn lower_src(src: &str) -> Result<Program, Diagnostic> {
        lower(&parse(src).expect("should parse"))
    }

    fn error(src: &str) -> String {
        lower_src(src).expect_err("should be refused").message
    }

    fn main_fn(src: &str) -> Func {
        lower_src(src).expect("should check").funcs.pop().expect("a function")
    }

    // ---- resolution ----------------------------------------------------

    #[test]
    fn parameters_take_the_first_slots() {
        let f = main_fn("fn f(a: int, b: int) -> int { let c = a + b; return c; }");
        assert_eq!(f.n_params, 2);
        assert_eq!(f.n_slots(), 3);
        assert_eq!(
            f.body[0],
            Stmt::Store {
                slot: Slot(2),
                value: Expr::Bin {
                    op: BinOp::Add,
                    lhs: Box::new(Expr::Load(Slot(0))),
                    rhs: Box::new(Expr::Load(Slot(1))),
                }
            }
        );
    }

    #[test]
    fn functions_are_visible_before_they_are_defined() {
        let p = lower_src("fn f() -> int { return g(); } fn g() -> int { return 1; }").unwrap();
        assert_eq!(p.funcs.len(), 2);
        assert_eq!(p.find("g"), Some(FuncId(1)));
    }

    #[test]
    fn an_inner_block_may_shadow() {
        let f =
            main_fn("fn f() -> int { let x = 1; if true { let x = 2; putchar(x); } return x; }");
        assert_eq!(f.n_slots(), 2);
    }

    #[test]
    fn rebinding_in_the_same_block_is_refused() {
        assert!(
            error("fn f() -> int { let x = 1; let x = 2; return x; }").contains("already bound")
        );
    }

    #[test]
    fn an_initialiser_cannot_see_its_own_binding() {
        assert!(error("fn f() -> int { let x = x; return x; }").contains("not bound"));
    }

    #[test]
    fn assigning_to_a_let_is_refused() {
        assert!(error("fn f() -> int { let x = 1; x = 2; return x; }").contains("immutable"));
    }

    #[test]
    fn assigning_to_a_parameter_is_refused() {
        assert!(error("fn f(a: int) -> int { a = 1; return a; }").contains("immutable"));
    }

    #[test]
    fn assigning_to_a_var_is_allowed() {
        assert!(lower_src("fn f() -> int { var x = 1; x = 2; return x; }").is_ok());
    }

    #[test]
    fn unknown_names_are_refused() {
        assert!(error("fn f() -> int { return nope; }").contains("not bound"));
        assert!(error("fn f() -> int { return nope(); }").contains("not a function"));
    }

    #[test]
    fn arity_is_checked_for_functions_and_builtins() {
        assert!(
            error("fn g(a: int) -> int { return a; } fn f() -> int { return g(); }")
                .contains("takes 1 argument")
        );
        assert!(error("fn f() -> int { return putchar(); }").contains("takes 1 argument"));
    }

    #[test]
    fn a_function_is_not_a_value() {
        assert!(
            error("fn g() -> int { return 1; } fn f() -> int { return g; }")
                .contains("no function values")
        );
    }

    #[test]
    fn a_local_is_not_callable() {
        assert!(error("fn f() -> int { let g = 1; return g(); }").contains("not a function"));
    }

    #[test]
    fn duplicate_definitions_are_refused() {
        assert!(
            error("fn f() -> int { return 1; } fn f() -> int { return 2; }")
                .contains("defined twice")
        );
        assert!(error("fn f(a: int, a: int) -> int { return a; }").contains("bound twice"));
    }

    #[test]
    fn builtins_cannot_be_redefined() {
        assert!(error("fn putchar(c: int) -> int { return c; }").contains("builtin"));
    }

    // ---- control flow --------------------------------------------------

    #[test]
    fn every_path_must_return() {
        assert!(error("fn f() -> int { let x = 1; }").contains("without returning"));
        assert!(error("fn f() -> int { if true { return 1; } }").contains("without returning"));
        assert!(lower_src("fn f() -> int { if true { return 1; } else { return 2; } }").is_ok());
        // Conservative on purpose: a loop never counts as a terminator.
        assert!(error("fn f() -> int { while true { } }").contains("without returning"));
    }

    #[test]
    fn code_after_a_return_is_refused() {
        assert!(error("fn f() -> int { return 1; return 2; }").contains("unreachable"));
    }

    // ---- types ---------------------------------------------------------

    #[test]
    fn slot_types_are_recorded_for_the_backend() {
        let f = main_fn("fn f(a: int, b: bool) -> int { let c = b; let d = a; return d; }");
        assert_eq!(f.slots, vec![Type::Int, Type::Bool, Type::Bool, Type::Int]);
        assert_eq!(f.ret, Type::Int);
    }

    #[test]
    fn a_let_takes_its_type_from_its_initialiser() {
        let f = main_fn("fn f() -> bool { let x = 1 < 2; return x; }");
        assert_eq!(f.slots, vec![Type::Bool]);
    }

    #[test]
    fn an_annotation_must_agree_with_the_initialiser() {
        assert!(
            error("fn f() -> int { let x: int = true; return x; }")
                .contains("expected `int`, found `bool`")
        );
        assert!(lower_src("fn f() -> bool { let x: bool = true; return x; }").is_ok());
    }

    #[test]
    fn a_condition_must_be_a_bool() {
        assert!(
            error("fn f() -> int { if 1 { return 0; } return 1; }")
                .contains("expected `bool`, found `int`")
        );
        assert!(
            error("fn f() -> int { while 1 { } return 1; }")
                .contains("expected `bool`, found `int`")
        );
    }

    #[test]
    fn return_must_match_the_signature() {
        assert!(error("fn f() -> int { return true; }").contains("expected `int`, found `bool`"));
        assert!(error("fn f() -> bool { return 1; }").contains("expected `bool`, found `int`"));
    }

    #[test]
    fn an_argument_must_match_the_parameter() {
        assert!(
            error("fn f() -> int { return putchar(true); }")
                .contains("expected `int`, found `bool`")
        );
    }

    #[test]
    fn an_assignment_must_match_the_binding() {
        assert!(
            error("fn f() -> int { var x = 1; x = true; return x; }")
                .contains("expected `int`, found `bool`")
        );
    }

    #[test]
    fn arithmetic_is_for_ints_and_logic_is_for_bools() {
        assert!(error("fn f() -> int { return true + true; }").contains("expected `int`"));
        assert!(error("fn f() -> bool { return 1 && 2; }").contains("expected `bool`"));
        assert!(error("fn f() -> int { return -true; }").contains("expected `int`"));
        assert!(error("fn f() -> bool { return !1; }").contains("expected `bool`"));
    }

    #[test]
    fn a_comparison_yields_a_bool() {
        let f = main_fn("fn f() -> bool { return 1 < 2; }");
        assert_eq!(f.ret, Type::Bool);
        // ...and ordering is for ints only.
        assert!(error("fn f() -> bool { return true < false; }").contains("expected `int`"));
    }

    #[test]
    fn equality_compares_two_values_of_the_same_type() {
        assert!(lower_src("fn f() -> bool { return 1 == 2; }").is_ok());
        assert!(lower_src("fn f() -> bool { return true == false; }").is_ok());
        assert!(error("fn f() -> bool { return 1 == true; }").contains("expected `int`"));
    }

    #[test]
    fn unknown_types_are_refused_where_they_are_written() {
        assert!(error("fn f() -> i32 { return 0; }").contains("unknown type `i32`"));
        assert!(error("fn f(a: i32) -> int { return 0; }").contains("unknown type `i32`"));
        assert!(
            error("fn f() -> int { let x: i32 = 1; return x; }").contains("unknown type `i32`")
        );
    }

    #[test]
    fn a_primitive_takes_no_type_arguments() {
        assert!(error("fn f() -> int[bool] { return 0; }").contains("takes no type arguments"));
    }

    // ---- structs -------------------------------------------------------

    #[test]
    fn a_struct_literal_is_reordered_into_declaration_order() {
        // Written y-then-x; the IR holds x-then-y, so the backend never has
        // to consult a field name.
        let f = main_fn("struct P { x: int, y: bool } fn f() -> P { return P { y: true, x: 7 }; }");
        let Stmt::Return(Expr::Struct { fields, .. }) = &f.body[0] else { panic!("{:?}", f.body) };
        assert_eq!(fields[0], Expr::Int(7));
        assert_eq!(fields[1], Expr::Bool(true));
    }

    #[test]
    fn a_field_access_becomes_an_index() {
        let f = main_fn("struct P { x: int, y: int } fn f(p: P) -> int { return p.y; }");
        let Stmt::Return(Expr::Field { index, .. }) = &f.body[0] else { panic!() };
        assert_eq!(*index, 1);
    }

    #[test]
    fn a_struct_literal_must_give_every_field_exactly_once() {
        let decl = "struct P { x: int, y: int } ";
        assert!(
            error(&format!("{decl}fn f() -> P {{ return P {{ x: 1 }}; }}"))
                .contains("missing field `y`")
        );
        assert!(
            error(&format!("{decl}fn f() -> P {{ return P {{ x: 1, y: 2, z: 3 }}; }}"))
                .contains("has no field `z`")
        );
        assert!(
            error(&format!("{decl}fn f() -> P {{ return P {{ x: 1, x: 2, y: 3 }}; }}"))
                .contains("given twice")
        );
        assert!(
            error(&format!("{decl}fn f() -> P {{ return P {{ x: true, y: 2 }}; }}"))
                .contains("expected `int`, found `bool`")
        );
    }

    #[test]
    fn fields_are_checked_against_the_declaration() {
        assert!(
            error("struct P { x: int } fn f(p: P) -> int { return p.z; }")
                .contains("`P` has no field `z`")
        );
        assert!(error("fn f() -> int { let x = 1; return x.y; }").contains("`int` has no fields"));
    }

    #[test]
    fn structs_may_nest_and_be_passed_by_value() {
        let p = lower_src(
            "struct P { x: int } struct L { a: P, b: P }              fn mid(l: L) -> int { return (l.a.x + l.b.x) / 2; }              fn f() -> int { return mid(L { a: P { x: 1 }, b: P { x: 3 } }); }",
        );
        assert!(p.is_ok(), "{:?}", p.err());
    }

    #[test]
    fn a_struct_that_contains_itself_is_refused() {
        // No references in M1, so this has no finite size. Both the direct and
        // the mutual case, because the check is reachability rather than a
        // look at one field.
        assert!(
            error("struct N { next: N } fn f() -> int { return 0; }").contains("contains itself")
        );
        assert!(
            error("struct A { b: B } struct B { a: A } fn f() -> int { return 0; }")
                .contains("contains itself")
        );
        // ...but two fields of the same struct type are perfectly finite.
        assert!(
            lower_src("struct P { x: int } struct L { a: P, b: P } fn f() -> int { return 0; }")
                .is_ok()
        );
    }

    #[test]
    fn struct_declarations_are_checked_for_duplicates() {
        assert!(
            error("struct P { x: int } struct P { y: int } fn f() -> int { return 0; }")
                .contains("declared twice")
        );
        assert!(
            error("struct P { x: int, x: bool } fn f() -> int { return 0; }")
                .contains("field `x` is declared twice")
        );
        assert!(
            error("struct int { x: int } fn f() -> int { return 0; }").contains("built-in type")
        );
    }

    #[test]
    fn a_struct_may_mention_one_declared_later() {
        assert!(
            lower_src("struct A { b: B } struct B { x: int } fn f() -> int { return 0; }").is_ok()
        );
    }

    #[test]
    fn structs_are_not_compared_with_equality() {
        assert!(
            error("struct P { x: int } fn f(a: P, b: P) -> bool { return a == b; }")
                .contains("cannot be compared")
        );
    }

    // ---- enums and match -------------------------------------------------

    const SHAPE: &str = "enum Shape { Empty, Circle(int), Rect(int, int) } ";

    #[test]
    fn a_variant_becomes_an_index_and_a_payload() {
        let f = main_fn(&format!("{SHAPE}fn f() -> Shape {{ return Shape::Rect(2, 3); }}"));
        let Stmt::Return(Expr::Enum { variant, payload, .. }) = &f.body[0] else { panic!() };
        assert_eq!(*variant, 2);
        assert_eq!(payload, &vec![Expr::Int(2), Expr::Int(3)]);
    }

    #[test]
    fn a_match_must_cover_every_variant() {
        let message = error(&format!(
            "{SHAPE}fn f(s: Shape) -> int {{ match s {{ Shape::Empty => {{ return 0; }} }} }}"
        ));
        assert!(message.contains("does not cover"), "{message}");
        assert!(message.contains("`Shape::Circle`"), "{message}");
        assert!(message.contains("`Shape::Rect`"), "{message}");
    }

    #[test]
    fn a_wildcard_covers_the_rest() {
        assert!(
            lower_src(&format!(
                "{SHAPE}fn f(s: Shape) -> int {{ match s {{ Shape::Empty => {{ return 0; }} _ => {{ return 1; }} }} }}"
            ))
            .is_ok()
        );
    }

    #[test]
    fn a_wildcard_that_covers_nothing_is_refused() {
        // Every variant is already matched, so the `_` can never run. Saying
        // so is worth more than silently allowing dead code.
        let message = error(&format!(
            "{SHAPE}fn f(s: Shape) -> int {{ match s {{              Shape::Empty => {{ return 0; }} Shape::Circle(r) => {{ return r; }}              Shape::Rect(w, h) => {{ return w * h; }} _ => {{ return 9; }} }} }}"
        ));
        assert!(message.contains("already matched"), "{message}");
    }

    #[test]
    fn arms_after_a_wildcard_are_refused() {
        let message = error(&format!(
            "{SHAPE}fn f(s: Shape) -> int {{ match s {{ _ => {{ return 0; }} Shape::Empty => {{ return 1; }} }} }}"
        ));
        assert!(message.contains("unreachable"), "{message}");
    }

    #[test]
    fn a_variant_may_not_be_matched_twice() {
        let message = error(&format!(
            "{SHAPE}fn f(s: Shape) -> int {{ match s {{              Shape::Empty => {{ return 0; }} Shape::Empty => {{ return 1; }} _ => {{ return 2; }} }} }}"
        ));
        assert!(message.contains("matched twice"), "{message}");
    }

    #[test]
    fn payload_arity_is_checked_when_building_and_when_matching() {
        assert!(
            error(&format!("{SHAPE}fn f() -> Shape {{ return Shape::Rect(1); }}"))
                .contains("carries 2 values, but 1 was given")
        );
        let message = error(&format!(
            "{SHAPE}fn f(s: Shape) -> int {{ match s {{ Shape::Rect(w) => {{ return w; }} _ => {{ return 0; }} }} }}"
        ));
        assert!(message.contains("the pattern binds 1"), "{message}");
    }

    #[test]
    fn a_binding_takes_the_payload_type() {
        // `r` is an `int`, so returning it from an `int` function is fine and
        // returning it from a `bool` one is not.
        assert!(
            lower_src(&format!(
                "{SHAPE}fn f(s: Shape) -> int {{ match s {{ Shape::Circle(r) => {{ return r; }} _ => {{ return 0; }} }} }}"
            ))
            .is_ok()
        );
        assert!(
            error(&format!(
                "{SHAPE}fn f(s: Shape) -> bool {{ match s {{ Shape::Circle(r) => {{ return r; }} _ => {{ return true; }} }} }}"
            ))
            .contains("expected `bool`, found `int`")
        );
    }

    #[test]
    fn two_arms_may_bind_the_same_name_at_different_types() {
        assert!(
            lower_src(
                "enum E { A(int), B(bool) }                  fn f(e: E) -> int { match e { E::A(v) => { return v; } E::B(v) => { if v { return 1; } return 0; } } }"
            )
            .is_ok()
        );
    }

    #[test]
    fn an_exhaustive_match_where_every_arm_returns_is_a_terminator() {
        // No trailing `return` needed: the match itself covers every path.
        assert!(
            lower_src(&format!(
                "{SHAPE}fn f(s: Shape) -> int {{ match s {{                  Shape::Empty => {{ return 0; }} Shape::Circle(r) => {{ return r; }}                  Shape::Rect(w, h) => {{ return w * h; }} }} }}"
            ))
            .is_ok()
        );
    }

    #[test]
    fn only_enums_are_matched() {
        assert!(
            error("fn f() -> int { match 1 { _ => { return 0; } } }")
                .contains("`int` cannot be matched")
        );
        assert!(
            error("struct P { x: int } fn f(p: P) -> int { match p { _ => { return 0; } } }")
                .contains("is a struct, not an enum")
        );
    }

    #[test]
    fn an_enum_that_contains_itself_is_refused() {
        assert!(
            error("enum List { Nil, Cons(int, List) } fn f() -> int { return 0; }")
                .contains("contains itself")
        );
    }

    #[test]
    fn an_enum_needs_at_least_one_variant() {
        assert!(error("enum Void { } fn f() -> int { return 0; }").contains("has no variants"));
    }

    #[test]
    fn structs_and_enums_are_not_interchangeable() {
        assert!(
            error("struct P { x: int } fn f() -> int { let p = P::x(1); return 0; }")
                .contains("is a struct, not an enum")
        );
        assert!(
            error("enum E { A } fn f() -> int { let e = E { x: 1 }; return 0; }")
                .contains("is an enum, not a struct")
        );
        assert!(
            error("enum E { A(int) } fn f(e: E) -> int { return e.x; }")
                .contains("read by matching on it")
        );
    }

    #[test]
    fn a_signature_is_checked_against_never_a_body() {
        // `g`'s body returns a bool, and its signature says int. The call in
        // `f` is checked against the signature, so the error is reported in
        // `g` -- a caller never learns anything from a callee's body.
        let message = error("fn g() -> int { return true; } fn f() -> int { return g(); }");
        assert!(message.contains("expected `int`, found `bool`"), "{message}");
    }
}
