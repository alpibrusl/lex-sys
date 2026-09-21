//! Compile-time evaluation (`docs/compile-time.md`).
//!
//! Two halves, and they live apart for one reason: **spans**.
//!
//! [`operator`] runs *during lowering*, where the AST's span is still in
//! hand, so a constant that cannot produce a value is refused where it is
//! written (§4). [`evaluate_calls`] runs *after* lowering, over the whole
//! program, because a call may name a function declared later in the
//! file — and the IR carries no spans, so a call that traps is left
//! alone to trap at run time rather than reported at no location (§9).
//!
//! What makes any of this legitimate is §6: an expression has one value
//! and it is the same value on every host and every target. Sixty-four
//! bit integers that trap rather than wrap, IEEE-754 binary64 with no
//! reassociation and no contraction, and nothing implementation-defined.
//! A C compiler folding at compile time has to worry that the host and
//! the target disagree; this one structurally cannot.

use lex_sys_types::Type;

use crate::{BinOp, Callee, Expr, Func, Program, Stmt};

/// What an evaluation came to.
///
/// `Trapped` and `Unknown` are deliberately different. A trap is a fact
/// about the program — the operation has no value and never will — and
/// §4 refuses it. `Unknown` means the evaluator declined, which is never
/// an error and never observable: the expression stays as it was and
/// runs at run time exactly as it does today (§5).
pub(crate) enum Folded {
    Value(Expr),
    Trapped(&'static str),
    Unknown,
}

fn int(e: &Expr) -> Option<i64> {
    match e {
        Expr::Int(n) => Some(*n),
        _ => None,
    }
}

fn float(e: &Expr) -> Option<f64> {
    match e {
        Expr::Float(bits) => Some(f64::from_bits(*bits)),
        _ => None,
    }
}

fn boolean(e: &Expr) -> Option<bool> {
    match e {
        Expr::Bool(b) => Some(*b),
        _ => None,
    }
}

fn of_float(x: f64) -> Expr {
    Expr::Float(x.to_bits())
}

/// Fold `op` over two operands, if both are already literals.
///
/// Every trap the backend would emit is reproduced here, from the same
/// reading of the same rules: overflow on `+ - * -x`, a zero divisor and
/// `int::MIN / -1` on `/` and `%`, and a shift amount outside `0..64`.
/// Getting one of these wrong would be the silently-wrong answer the
/// language exists to refuse, so `bin_traps_exactly_where_the_backend_does`
/// checks them against a running program rather than against this comment.
pub(crate) fn bin(op: BinOp, lhs: &Expr, rhs: &Expr) -> Folded {
    if let (Some(a), Some(b)) = (int(lhs), int(rhs)) {
        return int_bin(op, a, b);
    }
    if let (Some(a), Some(b)) = (float(lhs), float(rhs)) {
        return float_bin(op, a, b);
    }
    if let (Some(a), Some(b)) = (boolean(lhs), boolean(rhs)) {
        return match op {
            BinOp::Eq => Folded::Value(Expr::Bool(a == b)),
            BinOp::Ne => Folded::Value(Expr::Bool(a != b)),
            // Sound to fold even though these short-circuit: the right
            // operand is a literal, so there is nothing it could have
            // declined to do.
            BinOp::And => Folded::Value(Expr::Bool(a && b)),
            BinOp::Or => Folded::Value(Expr::Bool(a || b)),
            _ => Folded::Unknown,
        };
    }
    Folded::Unknown
}

fn int_bin(op: BinOp, a: i64, b: i64) -> Folded {
    let checked = |v: Option<i64>| match v {
        Some(n) => Folded::Value(Expr::Int(n)),
        None => Folded::Trapped("this arithmetic overflows"),
    };
    match op {
        BinOp::Add => checked(a.checked_add(b)),
        BinOp::Sub => checked(a.checked_sub(b)),
        BinOp::Mul => checked(a.checked_mul(b)),
        // Two inputs trap, and `checked_div` answers `None` for both:
        // a zero divisor, and `int::MIN / -1`, whose quotient is one
        // past the top of the range.
        BinOp::Div if b == 0 => Folded::Trapped("this divides by zero"),
        BinOp::Rem if b == 0 => Folded::Trapped("this divides by zero"),
        BinOp::Div => checked(a.checked_div(b)),
        BinOp::Rem => checked(a.checked_rem(b)),
        // `docs/bitwise.md` §3: the amount traps outside `0..64`, and a
        // negative amount is a huge unsigned one, which is why the
        // backend's check is a single unsigned comparison and this is
        // the same test spelled for a signed literal.
        BinOp::Shl | BinOp::Shr if !(0..64).contains(&b) => {
            Folded::Trapped("this shifts by an amount outside `0..64`")
        }
        // §4: a shift does not trap on the value it produces.
        BinOp::Shl => Folded::Value(Expr::Int(((a as u64) << b) as i64)),
        BinOp::Shr => Folded::Value(Expr::Int(a >> b)),
        BinOp::BitAnd => Folded::Value(Expr::Int(a & b)),
        BinOp::BitOr => Folded::Value(Expr::Int(a | b)),
        BinOp::BitXor => Folded::Value(Expr::Int(a ^ b)),
        BinOp::Eq => Folded::Value(Expr::Bool(a == b)),
        BinOp::Ne => Folded::Value(Expr::Bool(a != b)),
        BinOp::Lt => Folded::Value(Expr::Bool(a < b)),
        BinOp::Le => Folded::Value(Expr::Bool(a <= b)),
        BinOp::Gt => Folded::Value(Expr::Bool(a > b)),
        BinOp::Ge => Folded::Value(Expr::Bool(a >= b)),
        BinOp::And | BinOp::Or => Folded::Unknown,
    }
}

/// IEEE-754 binary64, which is what the type is (`floating-point.md` §1),
/// so Rust's `f64` *is* the semantics rather than a model of them.
///
/// Nothing here traps: division by zero is an infinity and `0.0 / 0.0` is
/// NaN, both values rather than errors (§2 of that document). The
/// comparisons inherit IEEE's unordered NaN, so `nan < nan` folds to
/// `false` and so does `nan >= nan` — which is the same answer the
/// backend's `fcmp` gives.
fn float_bin(op: BinOp, a: f64, b: f64) -> Folded {
    match op {
        BinOp::Add => Folded::Value(of_float(a + b)),
        BinOp::Sub => Folded::Value(of_float(a - b)),
        BinOp::Mul => Folded::Value(of_float(a * b)),
        BinOp::Div => Folded::Value(of_float(a / b)),
        BinOp::Eq => Folded::Value(Expr::Bool(a == b)),
        BinOp::Ne => Folded::Value(Expr::Bool(a != b)),
        BinOp::Lt => Folded::Value(Expr::Bool(a < b)),
        BinOp::Le => Folded::Value(Expr::Bool(a <= b)),
        BinOp::Gt => Folded::Value(Expr::Bool(a > b)),
        BinOp::Ge => Folded::Value(Expr::Bool(a >= b)),
        // `%` is not defined on floats (`floating-point.md` §3), and the
        // rest are not float operators at all.
        _ => Folded::Unknown,
    }
}

/// `-x`, `!b` and `~n` over a literal.
///
/// `-int::MIN` traps, for the same reason `std.math`'s `abs` does: the
/// answer is one past the top of the range and there is no honest number
/// to hand back.
pub(crate) fn negate(inner: &Expr, ty: &Type) -> Folded {
    match (int(inner), float(inner), ty) {
        (Some(n), _, Type::Int) => match n.checked_neg() {
            Some(v) => Folded::Value(Expr::Int(v)),
            None => Folded::Trapped("negating this overflows"),
        },
        (_, Some(x), Type::Float) => Folded::Value(of_float(-x)),
        _ => Folded::Unknown,
    }
}

pub(crate) fn not(inner: &Expr) -> Folded {
    match boolean(inner) {
        Some(b) => Folded::Value(Expr::Bool(!b)),
        None => Folded::Unknown,
    }
}

pub(crate) fn bit_not(inner: &Expr) -> Folded {
    match int(inner) {
        Some(n) => Folded::Value(Expr::Int(!n)),
        None => Folded::Unknown,
    }
}

// ---------------------------------------------------------------------
// Calls
// ---------------------------------------------------------------------

/// How many steps one call site may spend before the evaluator gives up
/// (§5).
///
/// Per *call site* rather than per program, so what a build produces does
/// not depend on the order functions were reached in. Running out is not
/// an error and never can be: the call is left as it was, and the program
/// computes the same answer a moment later.
const FUEL: u32 = 1_000_000;

/// How deep a compile-time call may nest. A pure function may recurse,
/// and a recursion that does not terminate would otherwise spend its
/// fuel on stack rather than on steps — and take the compiler's own
/// stack with it.
const DEPTH: u32 = 128;

#[derive(Debug)]
enum Stop {
    /// The function returned.
    Returned(Expr),
    /// The evaluator declined — an unsupported construct, a call it could
    /// not see into, or the fuel ran out. Never an error (§5).
    Give,
    /// A trap. The call would trap at run time with these arguments.
    /// Left unfolded rather than reported, because the IR has no span to
    /// point at (§9).
    Trapped,
}

struct Machine<'p> {
    program: &'p Program,
    fuel: u32,
    depth: u32,
}

/// Replace every call to a pure function on constant arguments with its
/// answer (§3).
///
/// Runs to a fixpoint across functions rather than once: folding a call
/// inside `f` can make a call to `f` foldable, and one pass over the
/// program in declaration order would find that only if the declarations
/// happened to be in the right order.
pub fn evaluate_calls(program: &mut Program) -> Tally {
    let mut folded = Tally::default();
    // A small bound rather than a loop until quiet: each round can only
    // turn calls into literals, so it terminates, and three rounds
    // reaches everything this repository's programs contain while
    // keeping a pathological input from making the compiler quadratic.
    for _ in 0..3 {
        let snapshot = program.clone();
        let mut here = Tally::default();
        for func in &mut program.funcs {
            fold_body(&mut func.body, &snapshot, &mut here);
        }
        let quiet = here.total() == 0;
        folded.calls += here.calls;
        folded.operators += here.operators;
        if quiet {
            break;
        }
    }
    folded
}

/// What one run of the pass removed.
///
/// Two numbers rather than one, because they answer different questions:
/// an operator folded here is one the *call* pass exposed — the lowering
/// already folded the ones that were literal in the source — and mixing
/// them would report a call and the arithmetic around it as two calls.
#[derive(Default, Clone, Copy, Debug)]
pub struct Tally {
    pub calls: usize,
    pub operators: usize,
}

impl Tally {
    fn total(self) -> usize {
        self.calls + self.operators
    }
}

fn fold_body(body: &mut [Stmt], program: &Program, tally: &mut Tally) {
    for stmt in body {
        match stmt {
            Stmt::Store { value, .. } | Stmt::Eval(value) | Stmt::Return(value) => {
                fold_in(value, program, tally)
            }
            Stmt::If { cond, then_body, else_body } => {
                fold_in(cond, program, tally);
                fold_body(then_body, program, tally);
                fold_body(else_body, program, tally);
            }
            Stmt::While { cond, body } => {
                fold_in(cond, program, tally);
                fold_body(body, program, tally);
            }
            Stmt::Borrow { body, .. } | Stmt::Region { body, .. } => {
                fold_body(body, program, tally)
            }
            Stmt::Match { scrutinee, arms, .. } => {
                fold_in(scrutinee, program, tally);
                for arm in arms {
                    fold_body(&mut arm.body, program, tally);
                }
            }
        }
    }
}

fn constant(e: &Expr) -> bool {
    matches!(e, Expr::Int(_) | Expr::Bool(_) | Expr::Float(_))
}

fn fold_in(e: &mut Expr, program: &Program, tally: &mut Tally) {
    // Children first, so a call whose argument is itself a foldable call
    // becomes foldable in the same pass.
    match e {
        Expr::Neg(a) | Expr::Not(a) | Expr::BitNot(a) | Expr::Len(a) => fold_in(a, program, tally),
        Expr::Bin { lhs, rhs, .. } => {
            fold_in(lhs, program, tally);
            fold_in(rhs, program, tally);
        }
        Expr::Call { args, .. } => {
            for a in args.iter_mut() {
                fold_in(a, program, tally);
            }
        }
        Expr::Index { base, index, .. } => {
            fold_in(base, program, tally);
            fold_in(index, program, tally);
        }
        Expr::Subslice { base, start, end, .. } => {
            fold_in(base, program, tally);
            fold_in(start, program, tally);
            fold_in(end, program, tally);
        }
        Expr::Struct { fields, .. } | Expr::Tuple { parts: fields, .. } => {
            for f in fields.iter_mut() {
                fold_in(f, program, tally);
            }
        }
        _ => {}
    }

    // Operators are folded during lowering, where a trap has a span to
    // point at. Doing it again here catches the ones a folded call just
    // exposed -- and those cannot be reported, so a trap gives up.
    let mut was_call = false;
    let replacement = match &*e {
        Expr::Bin { op, lhs, rhs } => match bin(*op, lhs, rhs) {
            Folded::Value(v) => Some(v),
            _ => None,
        },
        Expr::Call { callee: Callee::Fn(id), args }
            if !args.is_empty() && args.iter().all(constant) =>
        {
            let func = program.func(*id);
            if func.is_pure() {
                was_call = true;
                let mut machine = Machine { program, fuel: FUEL, depth: DEPTH };
                match machine.call(func, args.clone()) {
                    Stop::Returned(v) if constant(&v) => Some(v),
                    _ => None,
                }
            } else {
                None
            }
        }
        _ => None,
    };
    if let Some(v) = replacement {
        *e = v;
        if was_call {
            tally.calls += 1;
        } else {
            tally.operators += 1;
        }
    }
}

impl Machine<'_> {
    fn spend(&mut self) -> bool {
        match self.fuel.checked_sub(1) {
            Some(left) => {
                self.fuel = left;
                true
            }
            None => false,
        }
    }

    fn call(&mut self, func: &Func, args: Vec<Expr>) -> Stop {
        if self.depth == 0 {
            return Stop::Give;
        }
        // One slot per local, parameters first. `None` is "not written
        // yet", which a well-formed body never reads -- and if one did,
        // the evaluator gives up rather than inventing a zero.
        let mut slots: Vec<Option<Expr>> = vec![None; func.slots.len()];
        for (i, a) in args.into_iter().enumerate() {
            slots[i] = Some(a);
        }
        self.depth -= 1;
        let out = self.block(&func.body, &mut slots);
        self.depth += 1;
        match out {
            Some(stop) => stop,
            // Falling off the end of a body that should have returned:
            // the lowering guarantees it does not happen, and guessing a
            // value if it did is exactly the wrong repair.
            None => Stop::Give,
        }
    }

    /// `None` means the block finished without returning.
    fn block(&mut self, body: &[Stmt], slots: &mut Vec<Option<Expr>>) -> Option<Stop> {
        for stmt in body {
            if !self.spend() {
                return Some(Stop::Give);
            }
            match stmt {
                Stmt::Store { place: crate::Place::Slot(slot), value } => {
                    match self.eval(value, slots) {
                        Ok(v) => slots[slot.0 as usize] = Some(v),
                        Err(stop) => return Some(stop),
                    }
                }
                // Any other place writes through a reference or into
                // memory, which this evaluator has none of.
                Stmt::Store { .. } => return Some(Stop::Give),
                Stmt::Eval(value) => {
                    if let Err(stop) = self.eval(value, slots) {
                        return Some(stop);
                    }
                }
                Stmt::Return(value) => {
                    return Some(match self.eval(value, slots) {
                        Ok(v) => Stop::Returned(v),
                        Err(stop) => stop,
                    });
                }
                Stmt::If { cond, then_body, else_body } => {
                    let taken = match self.eval(cond, slots) {
                        Ok(Expr::Bool(b)) => b,
                        Ok(_) => return Some(Stop::Give),
                        Err(stop) => return Some(stop),
                    };
                    let branch = if taken { then_body } else { else_body };
                    if let Some(stop) = self.block(branch, slots) {
                        return Some(stop);
                    }
                }
                Stmt::While { cond, body } => loop {
                    if !self.spend() {
                        return Some(Stop::Give);
                    }
                    match self.eval(cond, slots) {
                        Ok(Expr::Bool(true)) => {}
                        Ok(Expr::Bool(false)) => break,
                        Ok(_) => return Some(Stop::Give),
                        Err(stop) => return Some(stop),
                    }
                    if let Some(stop) = self.block(body, slots) {
                        return Some(stop);
                    }
                },
                // A `region` holds an arena and a `borrow` holds a
                // reference; both are memory, and §3.1 keeps memory out
                // of compile-time evaluation until there is a document
                // for compile-time data.
                Stmt::Borrow { .. } | Stmt::Region { .. } | Stmt::Match { .. } => {
                    return Some(Stop::Give);
                }
            }
        }
        None
    }

    fn eval(&mut self, e: &Expr, slots: &mut Vec<Option<Expr>>) -> Result<Expr, Stop> {
        if !self.spend() {
            return Err(Stop::Give);
        }
        match e {
            Expr::Int(_) | Expr::Bool(_) | Expr::Float(_) => Ok(e.clone()),
            Expr::Load(slot) => slots[slot.0 as usize].clone().ok_or(Stop::Give),
            Expr::Neg(a) => {
                let v = self.eval(a, slots)?;
                // The type is not in hand here, so it comes from the
                // value: a literal is an `int` or a `float` and nothing
                // else can be negated.
                let ty = if matches!(v, Expr::Float(_)) { Type::Float } else { Type::Int };
                self.settle(negate(&v, &ty))
            }
            Expr::Not(a) => {
                let v = self.eval(a, slots)?;
                self.settle(not(&v))
            }
            Expr::BitNot(a) => {
                let v = self.eval(a, slots)?;
                self.settle(bit_not(&v))
            }
            Expr::Bin { op, lhs, rhs } => {
                // `&&` and `||` short-circuit, and the right operand may
                // be the thing that would trap, so they are evaluated as
                // control flow here exactly as the backend lowers them.
                if matches!(op, BinOp::And | BinOp::Or) {
                    let left = match self.eval(lhs, slots)? {
                        Expr::Bool(b) => b,
                        _ => return Err(Stop::Give),
                    };
                    if (*op == BinOp::And && !left) || (*op == BinOp::Or && left) {
                        return Ok(Expr::Bool(left));
                    }
                    return match self.eval(rhs, slots)? {
                        Expr::Bool(b) => Ok(Expr::Bool(b)),
                        _ => Err(Stop::Give),
                    };
                }
                let l = self.eval(lhs, slots)?;
                let r = self.eval(rhs, slots)?;
                self.settle(bin(*op, &l, &r))
            }
            Expr::Call { callee: Callee::Fn(id), args } => {
                let func = self.program.func(*id);
                if !func.is_pure() {
                    return Err(Stop::Give);
                }
                let mut values = Vec::with_capacity(args.len());
                for a in args {
                    values.push(self.eval(a, slots)?);
                }
                match self.call(func, values) {
                    Stop::Returned(v) => Ok(v),
                    other => Err(other),
                }
            }
            // Builtins, foreign calls, memory, aggregates: everything
            // else gives up. §3.1 -- an evaluator that declines is
            // invisible, and one that guesses is not.
            _ => Err(Stop::Give),
        }
    }

    fn settle(&mut self, folded: Folded) -> Result<Expr, Stop> {
        match folded {
            Folded::Value(v) => Ok(v),
            Folded::Trapped(_) => Err(Stop::Trapped),
            Folded::Unknown => Err(Stop::Give),
        }
    }
}
