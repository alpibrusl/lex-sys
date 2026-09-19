//! The M0 intermediate representation, and lowering from the AST.
//!
//! The IR keeps the AST's structured control flow but resolves everything a
//! backend would otherwise have to re-derive: names become dense local slots,
//! calls name a callee directly, and every arity and mutability question is
//! already settled. What reaches the backend cannot fail.
//!
//! This is where M0 does its checking. It is not a type checker — M0 has one
//! type — it is the resolution and well-formedness pass that M1's checker will
//! grow out of.

use lex_sys_syntax::ast::{
    self, Ast, Block, Expr as AstExpr, ExprId, Item, Stmt as AstStmt, StmtId, Symbol,
};
use lex_sys_syntax::span::Diagnostic;

/// A local variable: parameters occupy slots `0..n_params`, `let`/`var`
/// bindings take the slots after them, one per binding (never reused, so a
/// shadowing binding is simply a different slot).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot(pub u32);

/// Index into [`Program::funcs`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuncId(pub u32);

/// Functions the compiler provides rather than the program defining them.
///
/// M0 scaffolding: `putchar` is how a program produces output before there is
/// any FFI. M2 replaces it with a capability-gated foreign call — output is an
/// effect, and an effect must be granted (#1, #2). Until then a program can
/// write bytes without declaring anything, which is exactly the state of
/// affairs lex-sys exists to end.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Builtin {
    /// `putchar(c: int) -> int` — libc's, byte-for-byte.
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

    pub fn arity(self) -> usize {
        match self {
            Builtin::PutChar => 1,
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
    /// neither is undefined behaviour (#1).
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
}

impl BinOp {
    /// Comparisons yield `0` or `1`. M0 has no `bool`; M1 introduces one and
    /// this distinction stops being a convention and becomes a type.
    pub fn is_comparison(self) -> bool {
        matches!(self, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge)
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Callee {
    Fn(FuncId),
    Builtin(Builtin),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Expr {
    Const(i64),
    Load(Slot),
    Neg(Box<Expr>),
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
    /// Total slot count, parameters included.
    pub n_slots: u32,
    pub body: Vec<Stmt>,
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

/// Resolve and check an AST, producing IR a backend can lower without failing.
pub fn lower(ast: &Ast) -> Result<Program, Diagnostic> {
    // Pass 1: every function is visible to every other, so collect signatures
    // before lowering any body. Definition order in the file is irrelevant.
    let mut signatures: Vec<(Symbol, usize)> = Vec::new();
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
        if signatures.iter().any(|(sym, _)| *sym == decl.name) {
            return Err(Diagnostic::new(format!("function `{name}` is defined twice"), span));
        }

        let mut seen: Vec<Symbol> = Vec::new();
        for param in &decl.params {
            if seen.contains(&param.name) {
                return Err(Diagnostic::new(
                    format!("parameter `{}` is bound twice", ast.name_of(param.name)),
                    span,
                ));
            }
            seen.push(param.name);
        }

        signatures.push((decl.name, decl.params.len()));
    }

    let mut program = Program::default();
    for item in &ast.items {
        let Item::Fn(decl) = item;
        let mut f =
            FnLowering { ast, signatures: &signatures, scopes: vec![Vec::new()], n_slots: 0 };

        for param in &decl.params {
            // Parameters are immutable in M0: the shape of a binding that is
            // handed to you, not one you own outright.
            f.declare(param.name, false);
        }
        let body = f.block(&decl.body)?;

        if !terminates(&body) {
            return Err(Diagnostic::new(
                format!(
                    "function `{}` can finish without returning a value",
                    ast.name_of(decl.name)
                ),
                ast.item_span(ast::ItemId(program.funcs.len() as u32)),
            ));
        }

        program.funcs.push(Func {
            name: ast.name_of(decl.name).to_owned(),
            n_params: decl.params.len() as u32,
            n_slots: f.n_slots,
            body,
        });
    }

    Ok(program)
}

/// Does every path through these statements end in a `return`?
///
/// Deliberately structural and therefore conservative: a `while` never counts,
/// even `while 1 { }`. A conservative answer is a total one, and a program can
/// always say `return` once more.
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

struct Binding {
    name: Symbol,
    slot: Slot,
    mutable: bool,
}

struct FnLowering<'a> {
    ast: &'a Ast,
    signatures: &'a [(Symbol, usize)],
    scopes: Vec<Vec<Binding>>,
    n_slots: u32,
}

impl<'a> FnLowering<'a> {
    fn declare(&mut self, name: Symbol, mutable: bool) -> Slot {
        let slot = Slot(self.n_slots);
        self.n_slots += 1;
        self.scopes.last_mut().expect("a scope is always open").push(Binding {
            name,
            slot,
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
            AstStmt::Let { name, mutable, ty: _, value } => {
                // The initialiser is resolved *before* the binding exists, so
                // `let x = x;` reads the outer `x` or fails, and never itself.
                let value = self.expr(*value)?;
                if self.declared_in_current_scope(*name) {
                    return Err(Diagnostic::new(
                        format!(
                            "`{}` is already bound in this block (shadowing is only allowed in an inner block)",
                            self.ast.name_of(*name)
                        ),
                        span,
                    ));
                }
                let slot = self.declare(*name, *mutable);
                Stmt::Store { slot, value }
            }
            AstStmt::Assign { name, value } => {
                let value = self.expr(*value)?;
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
                Stmt::Store { slot: binding.slot, value }
            }
            AstStmt::Expr(e) => Stmt::Eval(self.expr(*e)?),
            AstStmt::If { cond, then_block, else_block } => {
                let cond = self.expr(*cond)?;
                let then_body = self.block(then_block)?;
                let else_body = match else_block {
                    Some(block) => self.block(block)?,
                    None => Vec::new(),
                };
                Stmt::If { cond, then_body, else_body }
            }
            AstStmt::While { cond, body } => {
                let cond = self.expr(*cond)?;
                let body = self.block(body)?;
                Stmt::While { cond, body }
            }
            AstStmt::Return(e) => Stmt::Return(self.expr(*e)?),
        })
    }

    fn expr(&mut self, id: ExprId) -> Result<Expr, Diagnostic> {
        let span = self.ast.expr_span(id);
        Ok(match self.ast.expr(id) {
            AstExpr::Int(v) => Expr::Const(*v),
            AstExpr::Name(name) => {
                let text = self.ast.name_of(*name);
                match self.lookup(*name) {
                    Some(binding) => Expr::Load(binding.slot),
                    None if self.signatures.iter().any(|(sym, _)| sym == name)
                        || Builtin::from_name(text).is_some() =>
                    {
                        return Err(Diagnostic::new(
                            format!(
                                "`{text}` is a function; M0 has no function values, so it can only be called"
                            ),
                            span,
                        ));
                    }
                    None => {
                        return Err(Diagnostic::new(format!("`{text}` is not bound here"), span));
                    }
                }
            }
            AstExpr::Unary { op: ast::UnOp::Neg, operand } => {
                Expr::Neg(Box::new(self.expr(*operand)?))
            }
            AstExpr::Binary { op, lhs, rhs } => Expr::Bin {
                op: bin_op(*op),
                lhs: Box::new(self.expr(*lhs)?),
                rhs: Box::new(self.expr(*rhs)?),
            },
            AstExpr::Call { callee, args } => {
                let text = self.ast.name_of(*callee);
                let lowered: Vec<Expr> =
                    args.iter().map(|&a| self.expr(a)).collect::<Result<_, _>>()?;

                if self.lookup(*callee).is_some() {
                    return Err(Diagnostic::new(
                        format!("`{text}` is a local binding, not a function"),
                        span,
                    ));
                }

                let (callee, arity) = if let Some(builtin) = Builtin::from_name(text) {
                    (Callee::Builtin(builtin), builtin.arity())
                } else {
                    let index =
                        self.signatures.iter().position(|(sym, _)| sym == callee).ok_or_else(
                            || {
                                Diagnostic::new(
                                    format!("`{text}` is not a function in this unit"),
                                    span,
                                )
                            },
                        )?;
                    (Callee::Fn(FuncId(index as u32)), self.signatures[index].1)
                };

                if lowered.len() != arity {
                    return Err(Diagnostic::new(
                        format!(
                            "`{text}` takes {arity} argument{}, but {} {} given",
                            if arity == 1 { "" } else { "s" },
                            lowered.len(),
                            if lowered.len() == 1 { "was" } else { "were" }
                        ),
                        span,
                    ));
                }
                Expr::Call { callee, args: lowered }
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

    #[test]
    fn parameters_take_the_first_slots() {
        let p = lower_src("fn f(a: int, b: int) -> int { let c = a + b; return c; }").unwrap();
        let f = &p.funcs[0];
        assert_eq!(f.n_params, 2);
        assert_eq!(f.n_slots, 3);
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
        let p = lower_src("fn f() -> int { let x = 1; if 1 { let x = 2; putchar(x); } return x; }")
            .unwrap();
        assert_eq!(p.funcs[0].n_slots, 2);
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

    #[test]
    fn every_path_must_return() {
        assert!(error("fn f() -> int { let x = 1; }").contains("without returning"));
        assert!(error("fn f() -> int { if 1 { return 1; } }").contains("without returning"));
        assert!(lower_src("fn f() -> int { if 1 { return 1; } else { return 2; } }").is_ok());
        // Conservative on purpose: a loop never counts as a terminator.
        assert!(error("fn f() -> int { while 1 { } }").contains("without returning"));
    }

    #[test]
    fn code_after_a_return_is_refused() {
        assert!(error("fn f() -> int { return 1; return 2; }").contains("unreachable"));
    }
}
