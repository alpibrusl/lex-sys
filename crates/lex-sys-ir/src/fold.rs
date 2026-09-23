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

use crate::{BinOp, Callee, Expr, Func, Place, Program, STATIC_ARENA, Stmt};

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
/// language exists to refuse, so `the_folder_agrees_with_the_backend`
/// checks every operator on every pair of boundary operands against a
/// running program rather than against this comment.
///
/// This paragraph used to name `bin_traps_exactly_where_the_backend_does`,
/// a test that never existed. The disagreement it would have caught --
/// `int::MIN % -1` -- shipped, and was found by reading the disassembly
/// instead (`docs/differential.md` §1).
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
        // `a % -1` is **0**, including `int::MIN % -1`, and the backend
        // answers 0 there rather than trapping: the remainder is
        // representable even where the quotient is not. Rust's
        // `checked_rem` calls it overflow, so using it unchanged made the
        // evaluator refuse a program the runtime computes — the two
        // halves of one compiler giving two answers for one expression.
        // Found by `docs/emitted-checks.md` §4, and it corrects
        // `defined-behaviour.md` §2.3 along with this line.
        BinOp::Rem if b == -1 => Folded::Value(Expr::Int(0)),
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
///
/// 128 was never measured against what it exists to prevent
/// (`docs/fuzzing.md`): a `main` thread gets the OS default, 8 MiB on
/// Linux, but `cargo test` runs each test on its own thread at Rust's
/// smaller default, 2 MiB, and a debug build's uninlined frames are
/// large enough that `Machine::call` recursing 128 deep overflows that
/// stack before the depth check ever has a chance to refuse. Found by
/// the corpus fuzzer's own mutation (`mutants_never_crash_the_compiler`)
/// when adding a corpus file (#176) shifted, for the fixed seed, which
/// mutant landed in the fixed iteration budget — onto `fib(1_000_000)`,
/// nesting 128 real stack frames deep before giving up. Bisected on a
/// debug build at a 2 MiB stack: 68 survives, 72 does not. This is
/// under half that measured floor, and well above every recursion depth
/// a fixture here actually asks the evaluator to fold — `fib(23)`
/// (`crates/lex-sys/tests/conformance/compile_time.rs`) needs 23.
const DEPTH: u32 = 32;

/// A value while a `static` is being evaluated
/// (`docs/compile-time-data.md` §4).
///
/// Two shapes and no more: a scalar, which is the `Expr` the folder
/// already traffics in, and a **handle** into the store — a run of
/// elements, which is exactly what a slice is at run time as well.
#[derive(Clone, Debug)]
enum Val {
    Scalar(Expr),
    /// `at` is where the run starts in the store and `len` is how long it
    /// is, so `s[a..b]` is arithmetic on a handle and copies nothing —
    /// the same property `slicing.md` §1 gives the runtime one.
    Slice {
        at: usize,
        len: usize,
    },
}

impl Val {
    fn scalar(self) -> Result<Expr, Stop> {
        match self {
            Val::Scalar(e) => Ok(e),
            Val::Slice { .. } => Err(Stop::Give),
        }
    }
}

#[derive(Debug)]
enum Stop {
    /// The function returned.
    Returned(Val),
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
    /// Everything `alloc_slice[static]` has handed out, one element per
    /// entry, in allocation order. A `static` body's whole memory.
    store: Vec<Expr>,
    /// The `static`s already evaluated, so a later one can read an
    /// earlier one (§2).
    done: &'p [Vec<i64>],
}

impl Machine<'_> {
    /// A machine for folding a call: no memory, because
    /// `compile-time.md` §3.1 keeps memory out of that half.
    fn folding(program: &Program) -> Machine<'_> {
        Machine { program, fuel: FUEL, depth: DEPTH, store: Vec::new(), done: &[] }
    }
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
            // Purity is not the whole condition, and this is the half that
            // was missing: a folded call becomes a **literal**, and `Expr`'s
            // literals are `int`, `bool` and `float`. There is no `byte`
            // one, because `byte_of` is where a byte comes from
            // (`strings.md` §2) -- so folding a `byte`-returning call put
            // an `Expr::Int` where the backend expects one machine byte,
            // and `int_of(g(65))`, `g(65) == byte_of(66)` and returning it
            // from a `byte` function each failed the Cranelift verifier
            // with no span and no rule. Found by
            // `docs/emitted-checks.md` §1, writing a program that did it
            // by accident.
            //
            // Refusing to fold is the repair rather than inventing a byte
            // literal: a new literal node moves every hash
            // (`canonical-ast.md` §3), and this is the one call shape in
            // the repository that it costs.
            if func.is_pure() && matches!(func.ret, Type::Int | Type::Bool | Type::Float) {
                was_call = true;
                let mut machine = Machine::folding(program);
                let values = args.iter().cloned().map(Val::Scalar).collect();
                match machine.call(func, values) {
                    // A call that answers a *slice* is not folded: there
                    // is nowhere to put the bytes at a call site, which is
                    // the whole reason a `static` is an item
                    // (`docs/compile-time-data.md` §2).
                    Stop::Returned(Val::Scalar(v)) if constant(&v) => Some(v),
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

    fn call(&mut self, func: &Func, args: Vec<Val>) -> Stop {
        if self.depth == 0 {
            return Stop::Give;
        }
        // One slot per local, parameters first. `None` is "not written
        // yet", which a well-formed body never reads -- and if one did,
        // the evaluator gives up rather than inventing a zero.
        let mut slots: Vec<Option<Val>> = vec![None; func.slots.len()];
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
    fn block(&mut self, body: &[Stmt], slots: &mut Vec<Option<Val>>) -> Option<Stop> {
        for stmt in body {
            if !self.spend() {
                return Some(Stop::Give);
            }
            match stmt {
                Stmt::Store { place: Place::Slot(slot), value } => match self.eval(value, slots) {
                    Ok(v) => slots[slot.0 as usize] = Some(v),
                    Err(stop) => return Some(stop),
                },
                // `s[i] = v` — the one write through memory a `static`
                // body needs (`docs/compile-time-data.md` §4). The bounds
                // check is the runtime's, and failing it is a **compile
                // error**, because a trap the evaluator reaches is one the
                // program would reach every time (`compile-time.md` §4).
                Stmt::Store { place: Place::Element { base, index, .. }, value } => {
                    let target = match self.eval(base, slots) {
                        Ok(v) => v,
                        Err(stop) => return Some(stop),
                    };
                    let at = match self.eval(index, slots).and_then(Val::scalar) {
                        Ok(Expr::Int(n)) => n,
                        Ok(_) => return Some(Stop::Give),
                        Err(stop) => return Some(stop),
                    };
                    let value = match self.eval(value, slots).and_then(Val::scalar) {
                        Ok(v) => v,
                        Err(stop) => return Some(stop),
                    };
                    let Val::Slice { at: start, len } = target else {
                        return Some(Stop::Give);
                    };
                    if at < 0 || at as usize >= len {
                        return Some(Stop::Trapped);
                    }
                    self.store[start + at as usize] = value;
                }
                // Any other place writes through a reference or into a
                // struct field, which this evaluator has neither of.
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
                    let taken = match self.eval(cond, slots).and_then(Val::scalar) {
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
                    match self.eval(cond, slots).and_then(Val::scalar) {
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
                // reference; a `static` needs neither, because its own
                // allocations go straight to the store. `match` wants
                // enums, which §6 keeps out.
                Stmt::Borrow { .. } | Stmt::Region { .. } | Stmt::Match { .. } => {
                    return Some(Stop::Give);
                }
            }
        }
        None
    }

    /// Put a run of elements in the store and answer a handle to it.
    fn allocate(&mut self, values: impl IntoIterator<Item = Expr>) -> Val {
        let at = self.store.len();
        self.store.extend(values);
        Val::Slice { at, len: self.store.len() - at }
    }

    fn eval(&mut self, e: &Expr, slots: &mut Vec<Option<Val>>) -> Result<Val, Stop> {
        if !self.spend() {
            return Err(Stop::Give);
        }
        match e {
            Expr::Int(_) | Expr::Bool(_) | Expr::Float(_) => Ok(Val::Scalar(e.clone())),
            Expr::Load(slot) => slots[slot.0 as usize].clone().ok_or(Stop::Give),
            // A string literal is a run of bytes, so it becomes one: the
            // same handle an `alloc_slice` hands back, and `len` and
            // indexing then need no second code path.
            Expr::Bytes(text) => {
                let bytes: Vec<Expr> =
                    text.as_bytes().iter().map(|b| Expr::Int(i64::from(*b))).collect();
                if !self.spend_many(bytes.len()) {
                    return Err(Stop::Give);
                }
                Ok(self.allocate(bytes))
            }
            // A `static` declared earlier, read as the data it became.
            Expr::Static(index) => {
                let Some(values) = self.done.get(*index as usize) else {
                    return Err(Stop::Give);
                };
                if !self.spend_many(values.len()) {
                    return Err(Stop::Give);
                }
                let values: Vec<Expr> = values.iter().map(|v| Expr::Int(*v)).collect();
                Ok(self.allocate(values))
            }
            // §2.1's allocation, and the only one there is. A runtime
            // arena never reaches here: `region` gives up above.
            Expr::AllocSlice { arena, count, fill, .. } if *arena == STATIC_ARENA => {
                let count = match self.eval(count, slots).and_then(Val::scalar)? {
                    Expr::Int(n) => n,
                    _ => return Err(Stop::Give),
                };
                let fill = self.eval(fill, slots).and_then(Val::scalar)?;
                // A negative length traps at run time
                // (`defined-behaviour.md` §4), so it is a diagnostic here.
                if count < 0 {
                    return Err(Stop::Trapped);
                }
                if !self.spend_many(count as usize) {
                    return Err(Stop::Give);
                }
                Ok(self.allocate(std::iter::repeat_n(fill, count as usize)))
            }
            Expr::Len(inner) => match self.eval(inner, slots)? {
                Val::Slice { len, .. } => Ok(Val::Scalar(Expr::Int(len as i64))),
                Val::Scalar(_) => Err(Stop::Give),
            },
            Expr::Index { base, index, .. } => {
                let target = self.eval(base, slots)?;
                let at = match self.eval(index, slots).and_then(Val::scalar)? {
                    Expr::Int(n) => n,
                    _ => return Err(Stop::Give),
                };
                let Val::Slice { at: start, len } = target else {
                    return Err(Stop::Give);
                };
                if at < 0 || at as usize >= len {
                    return Err(Stop::Trapped);
                }
                Ok(Val::Scalar(self.store[start + at as usize].clone()))
            }
            // `s[a..b]` is arithmetic on a handle and copies nothing, the
            // same property the runtime one has (`slicing.md` §1).
            Expr::Subslice { base, start, end, .. } => {
                let target = self.eval(base, slots)?;
                let from = match self.eval(start, slots).and_then(Val::scalar)? {
                    Expr::Int(n) => n,
                    _ => return Err(Stop::Give),
                };
                let to = match self.eval(end, slots).and_then(Val::scalar)? {
                    Expr::Int(n) => n,
                    _ => return Err(Stop::Give),
                };
                let Val::Slice { at, len } = target else {
                    return Err(Stop::Give);
                };
                if from < 0 || to < from || to as usize > len {
                    return Err(Stop::Trapped);
                }
                Ok(Val::Slice { at: at + from as usize, len: (to - from) as usize })
            }
            Expr::Neg(a) => {
                let v = self.eval(a, slots).and_then(Val::scalar)?;
                // The type is not in hand here, so it comes from the
                // value: a literal is an `int` or a `float` and nothing
                // else can be negated.
                let ty = if matches!(v, Expr::Float(_)) { Type::Float } else { Type::Int };
                self.settle(negate(&v, &ty))
            }
            Expr::Not(a) => {
                let v = self.eval(a, slots).and_then(Val::scalar)?;
                self.settle(not(&v))
            }
            Expr::BitNot(a) => {
                let v = self.eval(a, slots).and_then(Val::scalar)?;
                self.settle(bit_not(&v))
            }
            Expr::Bin { op, lhs, rhs } => {
                // `&&` and `||` short-circuit, and the right operand may
                // be the thing that would trap, so they are evaluated as
                // control flow here exactly as the backend lowers them.
                if matches!(op, BinOp::And | BinOp::Or) {
                    let left = match self.eval(lhs, slots).and_then(Val::scalar)? {
                        Expr::Bool(b) => b,
                        _ => return Err(Stop::Give),
                    };
                    if (*op == BinOp::And && !left) || (*op == BinOp::Or && left) {
                        return Ok(Val::Scalar(Expr::Bool(left)));
                    }
                    return match self.eval(rhs, slots).and_then(Val::scalar)? {
                        Expr::Bool(b) => Ok(Val::Scalar(Expr::Bool(b))),
                        _ => Err(Stop::Give),
                    };
                }
                let l = self.eval(lhs, slots).and_then(Val::scalar)?;
                let r = self.eval(rhs, slots).and_then(Val::scalar)?;
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
            // `byte_of` and `int_of` are the two builtins a table build
            // cannot do without: the alphabet is `[byte]` and the table
            // is `[int]`, so every entry crosses between them.
            Expr::Call { callee: Callee::Builtin(b), args } if args.len() == 1 => {
                let v = self.eval(&args[0], slots).and_then(Val::scalar)?;
                match (b.name(), v) {
                    ("int_of", Expr::Int(n)) => Ok(Val::Scalar(Expr::Int(n))),
                    ("byte_of", Expr::Int(n)) => {
                        // Out of range traps (`strings.md` §2), so it is a
                        // diagnostic here.
                        if !(0..256).contains(&n) {
                            return Err(Stop::Trapped);
                        }
                        Ok(Val::Scalar(Expr::Int(n)))
                    }
                    _ => Err(Stop::Give),
                }
            }
            // Foreign calls, the heap, aggregates: everything else gives
            // up. §6 — an evaluator that declines is invisible, and one
            // that guesses is not.
            _ => Err(Stop::Give),
        }
    }

    /// Charge for a bulk operation, so a huge `alloc_slice` costs what it
    /// takes rather than one step.
    fn spend_many(&mut self, n: usize) -> bool {
        let n = u32::try_from(n).unwrap_or(u32::MAX);
        match self.fuel.checked_sub(n) {
            Some(left) => {
                self.fuel = left;
                true
            }
            None => false,
        }
    }

    fn settle(&mut self, folded: Folded) -> Result<Val, Stop> {
        match folded {
            Folded::Value(v) => Ok(Val::Scalar(v)),
            Folded::Trapped(_) => Err(Stop::Trapped),
            Folded::Unknown => Err(Stop::Give),
        }
    }
}

// ---------------------------------------------------------------------
// Statics
// ---------------------------------------------------------------------

/// How many steps one `static` may spend (`docs/compile-time-data.md` §3).
///
/// Twenty times the budget a folded call gets, and the reason is in §3: a
/// table is built once and read for the life of the program, so it is
/// worth more compile time than an expression that saves a few
/// instructions. It is also the budget whose exhaustion is *visible* —
/// a `static` that needs more does not compile — so it is set where a
/// table someone would plausibly write fits inside it.
const STATIC_FUEL: u32 = 20_000_000;

/// Run a `static`'s body and answer its elements
/// (`docs/compile-time-data.md` §3).
///
/// `Err` is a sentence for a diagnostic rather than a `Diagnostic`,
/// because the span belongs to the item and this module does not have it.
/// There is no `Ok(None)`: a `static` has no runtime fallback, so
/// declining is refusing — *refuse, don't downgrade*.
pub fn evaluate_static(
    program: &Program,
    body: &Func,
    done: &[Vec<i64>],
) -> Result<Vec<i64>, String> {
    let mut machine = Machine { program, fuel: STATIC_FUEL, depth: DEPTH, store: Vec::new(), done };
    match machine.call(body, Vec::new()) {
        Stop::Returned(Val::Slice { at, len }) => {
            let mut values = Vec::with_capacity(len);
            for cell in &machine.store[at..at + len] {
                match cell {
                    Expr::Int(n) => values.push(*n),
                    Expr::Bool(b) => values.push(i64::from(*b)),
                    Expr::Float(bits) => values.push(*bits as i64),
                    // Every write into the store goes through
                    // `Val::scalar`, so this is unreachable rather than
                    // merely unlikely -- and a wrong guess here would put
                    // the wrong bytes in the binary.
                    _ => return Err("its elements are not all scalars".to_owned()),
                }
            }
            Ok(values)
        }
        Stop::Returned(Val::Scalar(_)) => {
            Err("it answers a scalar, and a `static` holds a slice".to_owned())
        }
        Stop::Trapped => Err("it traps: the operation has no value, and a `static` \
                              that cannot be computed cannot be compiled"
            .to_owned()),
        Stop::Give => {
            Err("the evaluator cannot run it: either it needs more than the compile-time budget, \
             or it reaches something a `static` may not (`docs/compile-time-data.md` §6)"
                .to_owned())
        }
    }
}
