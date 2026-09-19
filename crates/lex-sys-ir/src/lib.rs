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
use lex_sys_types::{Type, Unifier, UnifyError};

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
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
    Call { callee: Callee, args: Vec<Expr> },
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
    Return(Expr),
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

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Program {
    pub funcs: Vec<Func>,
}

impl Program {
    pub fn func(&self, id: FuncId) -> &Func {
        &self.funcs[id.0 as usize]
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

/// Resolve and check an AST, producing IR a backend can lower without failing.
pub fn lower(ast: &Ast) -> Result<Program, Diagnostic> {
    let mut unifier = Unifier::new();

    // Pass 1: every function is visible to every other, so collect signatures
    // before checking any body. Definition order in the file is irrelevant,
    // and no body is ever consulted to type a call.
    let mut signatures: Vec<Signature> = Vec::new();
    for (index, item) in ast.items.iter().enumerate() {
        let Item::Fn(decl) = item;
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
            params.push(resolve_type(ast, &unifier, param.ty)?);
        }

        let ret = resolve_type(ast, &unifier, decl.ret)?;
        signatures.push(Signature { name: decl.name, params, ret });
    }

    let mut program = Program::default();
    for (index, item) in ast.items.iter().enumerate() {
        let Item::Fn(decl) = item;
        let signature = &signatures[index];

        let mut f = FnLowering {
            ast,
            signatures: &signatures,
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
fn resolve_type(ast: &Ast, _unifier: &Unifier, id: TypeId) -> Result<Type, Diagnostic> {
    let written = ast.ty(id);
    let name = ast.name_of(written.name);
    let span = ast.type_span(id);

    let ty = match name {
        "int" => Type::Int,
        "bool" => Type::Bool,
        other => {
            return Err(Diagnostic::new(
                format!("unknown type `{other}` (M1 has `int` and `bool`)"),
                span,
            ));
        }
    };

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
                        let declared = resolve_type(self.ast, self.unifier, *written)?;
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
            AstStmt::Return(e) => {
                let (value, found) = self.expr(*e)?;
                let ret = self.ret.clone();
                self.expect_type(&ret, &found, self.ast.expr_span(*e))?;
                Stmt::Return(value)
            }
        })
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
                    // `==` and `!=` compare any two values of the same type.
                    BinOp::Eq | BinOp::Ne => {}
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

    #[test]
    fn a_signature_is_checked_against_never_a_body() {
        // `g`'s body returns a bool, and its signature says int. The call in
        // `f` is checked against the signature, so the error is reported in
        // `g` -- a caller never learns anything from a callee's body.
        let message = error("fn g() -> int { return true; } fn f() -> int { return g(); }");
        assert!(message.contains("expected `int`, found `bool`"), "{message}");
    }
}
