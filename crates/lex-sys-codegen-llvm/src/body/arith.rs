//! Arithmetic and comparison: checked `int` operators, unchecked
//! `float` ones, and the two comparisons. `scalar_kind` is here
//! too -- the structural type inference `binop`/`Expr::Neg` need
//! to dispatch between the two (§7.17).

use crate::*;

impl<'a> FuncEmitter<'a> {
    pub(crate) fn scalar_kind(&self, expr: &Expr) -> Result<LKind, String> {
        match expr {
            Expr::Float(_) => Ok(LKind::F64),
            Expr::Int(_) => Ok(LKind::I64),
            Expr::Bool(_) => Ok(LKind::I8),
            Expr::Load(slot) => self
                .slot_kinds
                .get(slot.0 as usize)
                .and_then(|kinds| kinds.first())
                .copied()
                .ok_or_else(|| "a zero-leaf slot has no scalar kind".to_owned()),
            Expr::Neg(inner) | Expr::Not(inner) | Expr::BitNot(inner) => self.scalar_kind(inner),
            // A comparison or short-circuit op always answers `bool`;
            // every other `BinOp` answers whatever its operands are --
            // the checker already agreed `lhs`/`rhs` match, so `lhs`
            // decides, the same one-sided reasoning Cranelift's own
            // dynamic `value_type` check uses.
            Expr::Bin { op, lhs, .. } => {
                if op.is_comparison() || op.is_short_circuit() {
                    Ok(LKind::I8)
                } else {
                    self.scalar_kind(lhs)
                }
            }
            Expr::Field { def, args, index, .. } | Expr::FieldRef { def, args, index, .. } => {
                let (_, kinds) = self.field_offset(*def, args, *index)?;
                kinds
                    .into_iter()
                    .next()
                    .ok_or_else(|| "a zero-leaf field has no scalar kind".to_owned())
            }
            Expr::TupleField { components, index, .. }
            | Expr::TupleFieldRef { components, index, .. } => {
                let (_, kinds) = self.tuple_field_offset(components, *index)?;
                kinds
                    .into_iter()
                    .next()
                    .ok_or_else(|| "a zero-leaf tuple field has no scalar kind".to_owned())
            }
            Expr::Index { element, .. } => leaves_of(element, self.program)?
                .into_iter()
                .next()
                .ok_or_else(|| "a zero-leaf element has no scalar kind".to_owned()),
            Expr::Deref { ty, .. } | Expr::Contents { ty, .. } => leaves_of(ty, self.program)?
                .into_iter()
                .next()
                .ok_or_else(|| "a zero-leaf value has no scalar kind".to_owned()),
            // `len(s)`/`unbox_slice(h, b)` are their own nodes, not
            // `Callee::Builtin` calls (`lex-sys-codegen`'s own `Expr::Len`/
            // `Expr::UnboxedSlice` arms are `unreachable!()` inside
            // `call()` for the same reason) -- both always answer `int`.
            Expr::Len(_) | Expr::UnboxedSlice { .. } => Ok(LKind::I64),
            Expr::Call { callee, .. } => match callee {
                Callee::Builtin(Builtin::FloatOf | Builtin::Sqrt) => Ok(LKind::F64),
                // Every builtin below has one fixed, scalar return type
                // (`Builtin::signature`'s own match, `lex-sys-ir::
                // builtin.rs`) -- not a capability, not a type the call
                // site decides, and not multi-leaf.
                Callee::Builtin(
                    Builtin::PutChar
                    | Builtin::Write
                    | Builtin::WriteErr
                    | Builtin::GetChar
                    | Builtin::WrappingAdd
                    | Builtin::WrappingSub
                    | Builtin::WrappingMul
                    | Builtin::Close
                    | Builtin::ArgCount
                    | Builtin::IntOf
                    | Builtin::Truncate
                    | Builtin::BitsOf
                    | Builtin::Listen
                    | Builtin::Accept
                    | Builtin::Release,
                ) => Ok(LKind::I64),
                Callee::Builtin(Builtin::IsNan | Builtin::ByteOf) => Ok(LKind::I8),
                Callee::Fn(id) => {
                    let target = self.program.func(*id);
                    leaves_of(&target.ret, self.program)?
                        .into_iter()
                        .next()
                        .ok_or_else(|| "a zero-leaf return has no scalar kind".to_owned())
                }
                other => Err(format!("cannot determine the scalar kind of a `{other:?}` call")),
            },
            other => Err(format!("cannot determine the scalar kind of `{other:?}` here")),
        }
    }

    /// `docs/llvm-backend.md` §5: every `BinOp`. The two short-circuit
    /// logical operators are handled before either operand is evaluated
    /// -- `rhs` must not run when `lhs` already decided the answer, which
    /// is the one thing this function's usual "evaluate both, then
    /// dispatch" shape must not do here.
    pub(crate) fn binop(
        &mut self,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
    ) -> Result<Vec<LValue>, String> {
        if op.is_short_circuit() {
            return self.short_circuit(op, lhs, rhs);
        }
        let lhs_kind = self.scalar_kind(lhs)?;
        let a = self.scalar(lhs)?;
        let b = self.scalar(rhs)?;
        if lhs_kind == LKind::F64 {
            return self.float_binop(op, a, b);
        }
        match op {
            BinOp::Add => self.checked_arith("sadd", a, b),
            BinOp::Sub => self.checked_arith("ssub", a, b),
            BinOp::Mul => self.checked_arith("smul", a, b),
            BinOp::Div => self.checked_div(a, b, false),
            BinOp::Rem => self.checked_div(a, b, true),
            BinOp::Shl | BinOp::Shr => self.checked_shift(op, a, b),
            BinOp::BitAnd => Ok(vec![self.plain("and", a, b)]),
            BinOp::BitOr => Ok(vec![self.plain("or", a, b)]),
            BinOp::BitXor => Ok(vec![self.plain("xor", a, b)]),
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                Ok(vec![self.compare(op, a, b)])
            }
            BinOp::And | BinOp::Or => unreachable!("refused above"),
        }
    }

    /// `Add`/`Sub`/`Mul`/`Div`: plain IEEE-754, never checked -- a float
    /// has no analogue of the traps `checked_arith`/`checked_div` guard
    /// against (`docs/floating-point.md` §2). Comparisons are the
    /// ordered `fcmp` predicates, false whenever either side is NaN,
    /// except `!=`, which is `une` (unordered-or-not-equal) rather than
    /// the ordered `one` -- the one place `==`'s and `!=`'s IEEE
    /// semantics are not simple negations of each other, matching
    /// `IsNan`'s own `x != x` riddle below.
    pub(crate) fn float_binop(
        &mut self,
        op: BinOp,
        a: LValue,
        b: LValue,
    ) -> Result<Vec<LValue>, String> {
        let instr = match op {
            BinOp::Add => "fadd",
            BinOp::Sub => "fsub",
            BinOp::Mul => "fmul",
            BinOp::Div => "fdiv",
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                return Ok(vec![self.float_compare(op, a, b)]);
            }
            other => unreachable!("the checker refuses `{other:?}` on `float`"),
        };
        let result = self.fresh();
        self.out.push_str(&format!(
            "  {result} = {instr} double {}, {}\n",
            operand(&a),
            operand(&b)
        ));
        Ok(vec![LValue::Reg(result)])
    }

    pub(crate) fn float_compare(&mut self, op: BinOp, a: LValue, b: LValue) -> LValue {
        let cc = match op {
            BinOp::Eq => "oeq",
            BinOp::Ne => "une",
            BinOp::Lt => "olt",
            BinOp::Le => "ole",
            BinOp::Gt => "ogt",
            BinOp::Ge => "oge",
            other => unreachable!("`{other:?}` is not a comparison"),
        };
        let cmp = self.fresh();
        self.out.push_str(&format!(
            "  {cmp} = fcmp {cc} double {}, {}\n",
            operand(&a),
            operand(&b)
        ));
        let widened = self.fresh();
        self.out.push_str(&format!("  {widened} = zext i1 {cmp} to i8\n"));
        LValue::Reg(widened)
    }

    /// `Add`/`Sub`/`Mul`: LLVM's own overflow-reporting intrinsics, the
    /// direct counterpart of Cranelift's `sadd_overflow`/`ssub_overflow`/
    /// `smul_overflow` -- checked against a real `clang` in this slice's
    /// own session (`docs/llvm-backend.md` §5).
    pub(crate) fn checked_arith(
        &mut self,
        op: &str,
        a: LValue,
        b: LValue,
    ) -> Result<Vec<LValue>, String> {
        let pair = self.fresh();
        self.out.push_str(&format!(
            "  {pair} = call {{i64, i1}} @llvm.{op}.with.overflow.i64(i64 {}, i64 {})\n",
            operand(&a),
            operand(&b)
        ));
        let value = self.fresh();
        self.out.push_str(&format!("  {value} = extractvalue {{i64, i1}} {pair}, 0\n"));
        let overflowed = self.fresh();
        self.out.push_str(&format!("  {overflowed} = extractvalue {{i64, i1}} {pair}, 1\n"));
        self.trap_if(&overflowed)?;
        Ok(vec![LValue::Reg(value)])
    }

    /// `Div`/`Rem`: LLVM's `sdiv`/`srem` are **undefined**, not trapping,
    /// on a zero divisor or on `int::MIN / -1` -- unlike Cranelift's,
    /// which trap on both (`docs/defined-behaviour.md`). So both checks
    /// this backend needs are explicit, ahead of the instruction, rather
    /// than inherited from the instruction the way `checked_arith`'s is.
    pub(crate) fn checked_div(
        &mut self,
        a: LValue,
        b: LValue,
        remainder: bool,
    ) -> Result<Vec<LValue>, String> {
        let (a_op, b_op) = (operand(&a), operand(&b));
        let zero = self.fresh();
        self.out.push_str(&format!("  {zero} = icmp eq i64 {b_op}, 0\n"));
        self.trap_if(&zero)?;
        let is_min = self.fresh();
        self.out.push_str(&format!("  {is_min} = icmp eq i64 {a_op}, -9223372036854775808\n"));
        let is_neg1 = self.fresh();
        self.out.push_str(&format!("  {is_neg1} = icmp eq i64 {b_op}, -1\n"));
        let both = self.fresh();
        self.out.push_str(&format!("  {both} = and i1 {is_min}, {is_neg1}\n"));
        self.trap_if(&both)?;
        let result = self.fresh();
        let instr = if remainder { "srem" } else { "sdiv" };
        self.out.push_str(&format!("  {result} = {instr} i64 {a_op}, {b_op}\n"));
        Ok(vec![LValue::Reg(result)])
    }

    /// `Shl`/`Shr`: an amount outside `0..64` traps (`docs/bitwise.md`
    /// §3) rather than being masked the way LLVM's `shl`/`ashr` would
    /// silently do it. `uge` catches a negative amount the same way
    /// Cranelift's `UnsignedGreaterThanOrEqual` does: reinterpreted as
    /// unsigned, a negative `i64` is far past 64.
    pub(crate) fn checked_shift(
        &mut self,
        op: BinOp,
        a: LValue,
        b: LValue,
    ) -> Result<Vec<LValue>, String> {
        let (a_op, b_op) = (operand(&a), operand(&b));
        let out_of_range = self.fresh();
        self.out.push_str(&format!("  {out_of_range} = icmp uge i64 {b_op}, 64\n"));
        self.trap_if(&out_of_range)?;
        let result = self.fresh();
        // `ashr`, not `lshr`: `int` is signed (`docs/bitwise.md` §2), the
        // same reason Cranelift's `Shr` lowers to `sshr`.
        let instr = if op == BinOp::Shl { "shl" } else { "ashr" };
        self.out.push_str(&format!("  {result} = {instr} i64 {a_op}, {b_op}\n"));
        Ok(vec![LValue::Reg(result)])
    }

    /// `BitAnd`/`BitOr`/`BitXor`: none of the three can overflow
    /// (`docs/bitwise.md` §4), so unlike `checked_arith` there is nothing
    /// to check.
    pub(crate) fn plain(&mut self, instr: &str, a: LValue, b: LValue) -> LValue {
        let result = self.fresh();
        self.out.push_str(&format!("  {result} = {instr} i64 {}, {}\n", operand(&a), operand(&b)));
        LValue::Reg(result)
    }

    /// `wrapping_add`/`sub`/`mul`: two `int` leaves in, `plain` handles the
    /// rest -- `instr` is `"add"`/`"sub"`/`"mul"`, which doubles as the
    /// builtin's own name suffix for the error message.
    pub(crate) fn wrapping(
        &mut self,
        instr: &str,
        evaluated: Vec<Vec<LValue>>,
    ) -> Result<Vec<LValue>, String> {
        let mut flat = evaluated.into_iter().flatten();
        let a =
            flat.next().ok_or_else(|| format!("`wrapping_{instr}` needs two `int` arguments"))?;
        let b =
            flat.next().ok_or_else(|| format!("`wrapping_{instr}` needs two `int` arguments"))?;
        Ok(vec![self.plain(instr, a, b)])
    }

    /// The six comparisons. `icmp` yields `i1`; `zext`ed to `i8` because
    /// that is this backend's `bool` leaf, the same widening Cranelift's
    /// `icmp` needs none of (its `i8` result already is one).
    pub(crate) fn compare(&mut self, op: BinOp, a: LValue, b: LValue) -> LValue {
        let cc = match op {
            BinOp::Eq => "eq",
            BinOp::Ne => "ne",
            BinOp::Lt => "slt",
            BinOp::Le => "sle",
            BinOp::Gt => "sgt",
            BinOp::Ge => "sge",
            other => unreachable!("`{other:?}` is not a comparison"),
        };
        let cmp = self.fresh();
        self.out.push_str(&format!("  {cmp} = icmp {cc} i64 {}, {}\n", operand(&a), operand(&b)));
        let widened = self.fresh();
        self.out.push_str(&format!("  {widened} = zext i1 {cmp} to i8\n"));
        LValue::Reg(widened)
    }

    /// This backend's `bool` leaf is `i8` (0 or 1); every branch needs
    /// LLVM's own `i1`, which this is the one conversion for.
    pub(crate) fn short_circuit(
        &mut self,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
    ) -> Result<Vec<LValue>, String> {
        let a = self.scalar(lhs)?;
        let cond = self.truthy(&a);
        let n = self.blocks;
        self.blocks += 1;
        let (rhs_label, short_label, end_label) =
            (format!("sc_rhs{n}"), format!("sc_short{n}"), format!("sc_end{n}"));
        let slot = self.fresh();
        self.out.push_str(&format!("  {slot} = alloca i8\n"));
        // `&&`: a true `lhs` still needs `rhs`; a false one already
        // answered `false`, so the branch taken on `lhs == true` goes to
        // `rhs_label` and `lhs == false` goes to `short_label`. `||` is
        // the mirror of both: `lhs == true` already answered `true`, and
        // only `lhs == false` still needs `rhs`.
        let (on_true, on_false) = if op == BinOp::And {
            (rhs_label.clone(), short_label.clone())
        } else {
            (short_label.clone(), rhs_label.clone())
        };
        self.out.push_str(&format!("  br i1 {cond}, label %{on_true}, label %{on_false}\n"));
        self.out.push_str(&format!("{rhs_label}:\n"));
        let b = self.scalar(rhs)?;
        self.out.push_str(&format!("  store i8 {}, ptr {slot}\n", operand(&b)));
        self.out.push_str(&format!("  br label %{end_label}\n"));
        self.out.push_str(&format!("{short_label}:\n"));
        let shorted = if op == BinOp::And { 0 } else { 1 };
        self.out.push_str(&format!("  store i8 {shorted}, ptr {slot}\n"));
        self.out.push_str(&format!("  br label %{end_label}\n"));
        self.out.push_str(&format!("{end_label}:\n"));
        let result = self.fresh();
        self.out.push_str(&format!("  {result} = load i8, ptr {slot}\n"));
        Ok(vec![LValue::Reg(result)])
    }
}
