//! Lowering `lex_sys_ir::Program` to LLVM textual IR.
//!
//! Every lex-sys value is its leaves, exactly as `lex-sys-codegen`'s own
//! `abi.rs` scalarises one: a capability is zero leaves, a reference is one
//! pointer leaf (whatever it refers to), `int` is one `i64`. Unlike the
//! Cranelift path, which builds SSA `Variable`s that Cranelift itself
//! promotes out of memory, every leaf here is one `alloca`, written and
//! read with plain `store`/`load` -- `clang`'s mandatory `mem2reg` does the
//! promotion this crate does not bother building, which is the one place
//! shelling out to `clang` buys more than a linker.
//!
//! A failure returns `(Option<usize>, String)` -- the function index and a
//! message -- exactly the shape [`lex_sys_codegen::CodegenError`] wants,
//! without this crate depending on Cranelift to build it.

use lex_sys_ir::{BinOp, Builtin, Callee, Expr, Func, Place, Program, Slot, Stmt};
use lex_sys_types::Type;
use target_lexicon::Triple;

/// The machine types a leaf may be. No `f64`: `Type::Float` is refused
/// (§5's first slice has no arithmetic, and floats have none of it here).
#[derive(Clone, Copy, PartialEq, Eq)]
enum LKind {
    I64,
    I8,
    Ptr,
}

impl LKind {
    fn llvm(self) -> &'static str {
        match self {
            LKind::I64 => "i64",
            LKind::I8 => "i8",
            LKind::Ptr => "ptr",
        }
    }

    fn zero(self) -> &'static str {
        match self {
            LKind::I64 | LKind::I8 => "0",
            LKind::Ptr => "null",
        }
    }
}

/// An operand: a compile-time constant, printed inline, or a named
/// register a prior instruction produced. LLVM textual IR allows a
/// constant wherever a register is expected, so a literal never needs an
/// instruction of its own the way Cranelift's `iconst` does.
#[derive(Clone)]
enum LValue {
    Const(i64),
    Reg(String),
}

fn operand(v: &LValue) -> String {
    match v {
        LValue::Const(n) => n.to_string(),
        LValue::Reg(name) => name.clone(),
    }
}

/// The leaves a type scalarises to, matching `lex-sys-codegen`'s
/// `abi::leaves_into` for the subset this slice supports.
///
/// `Err` names what is missing rather than panicking: a type this function
/// refuses is a type the first slice does not lower yet, not a checker bug.
fn leaves_of(ty: &Type, program: &Program) -> Result<Vec<LKind>, String> {
    let mut out = Vec::new();
    leaves_into(ty, program, &mut out)?;
    Ok(out)
}

fn leaves_into(ty: &Type, program: &Program, out: &mut Vec<LKind>) -> Result<(), String> {
    match ty {
        Type::Int => out.push(LKind::I64),
        Type::Byte | Type::Bool => out.push(LKind::I8),
        Type::Ref { inner, .. } => {
            out.push(LKind::Ptr);
            if matches!(inner.as_ref(), Type::Slice(_)) {
                out.push(LKind::I64);
            }
        }
        Type::Tuple(parts) => {
            for part in parts {
                leaves_into(part, program, out)?;
            }
        }
        Type::Named(def, args) => match program.type_info(*def) {
            lex_sys_ir::TypeInfo::Struct { fields, .. } => {
                for (_, field) in fields {
                    leaves_into(&field.substitute(args, &[]), program, out)?;
                }
            }
            lex_sys_ir::TypeInfo::Enum { name, .. } => {
                return Err(format!(
                    "`{name}` is an enum, which the LLVM backend's first slice does not lower yet"
                ));
            }
        },
        other => {
            return Err(format!(
                "`{other:?}` is not part of the LLVM backend's first slice (docs/llvm-backend.md §5)"
            ));
        }
    }
    Ok(())
}

pub(crate) fn emit_module(
    program: &Program,
    entry: &str,
    triple: &Triple,
) -> Result<String, (Option<usize>, String)> {
    let mut text = String::new();
    text.push_str(&format!("target triple = \"{triple}\"\n\n"));
    text.push_str("declare i32 @putchar(i32)\n");
    // Checked arithmetic (§5's second slice): the three overflow-reporting
    // intrinsics `Expr::Bin`'s `Add`/`Sub`/`Mul` arms call. Declared
    // unconditionally, the same way `putchar` is -- an unused `declare`
    // costs nothing, and every function in the module shares one `.ll`.
    text.push_str("declare {i64, i1} @llvm.sadd.with.overflow.i64(i64, i64)\n");
    text.push_str("declare {i64, i1} @llvm.ssub.with.overflow.i64(i64, i64)\n");
    text.push_str("declare {i64, i1} @llvm.smul.with.overflow.i64(i64, i64)\n\n");

    for (index, func) in program.funcs.iter().enumerate() {
        let body = FuncEmitter::new(program, func, triple)
            .and_then(|mut fe| fe.emit())
            .map_err(|message| (Some(index), message))?;
        text.push_str(&body);
        text.push('\n');
    }

    let entry_id = program
        .find(entry)
        .ok_or_else(|| (None, format!("no function named `{entry}` to use as entry")))?;
    let entry_func = program.func(entry_id);
    let ret = leaves_of(&entry_func.ret, program).map_err(|m| (None, m))?;
    if ret.len() > 1 {
        return Err((None, "the entry point's return type has more than one leaf".to_owned()));
    }
    text.push_str("define i32 @main(i32 %argc, ptr %argv) {\n");
    text.push_str("entry:\n");
    match ret.first() {
        Some(LKind::I64) => {
            text.push_str(&format!("  %r = call i64 @lexs_{entry}()\n"));
            text.push_str("  %status = trunc i64 %r to i32\n");
            text.push_str("  ret i32 %status\n");
        }
        // `docs/agent-errors.md`'s own convention applied here too: a
        // located, worded refusal rather than a silent wrong exit code.
        _ => {
            return Err((None, "`main` must return `int`, the process exit status".to_owned()));
        }
    }
    text.push_str("}\n");

    Ok(text)
}

struct FuncEmitter<'a> {
    program: &'a Program,
    func: &'a Func,
    /// Per slot, the LLVM type of each of its leaves.
    slot_kinds: Vec<Vec<LKind>>,
    triple: &'a Triple,
    out: String,
    temp: u32,
    /// Numbers each `trap`/`ok` block pair a checked operator opens
    /// (§5's second slice) -- distinct from `temp`, which numbers SSA
    /// registers, because a block label and a register share no namespace
    /// in LLVM IR but reusing one counter for both would still be correct;
    /// two counters just read clearer in the emitted text.
    blocks: u32,
}

impl<'a> FuncEmitter<'a> {
    fn new(program: &'a Program, func: &'a Func, triple: &'a Triple) -> Result<Self, String> {
        let slot_kinds =
            func.slots.iter().map(|ty| leaves_of(ty, program)).collect::<Result<Vec<_>, _>>()?;
        Ok(FuncEmitter {
            program,
            func,
            slot_kinds,
            triple,
            out: String::new(),
            temp: 0,
            blocks: 0,
        })
    }

    fn fresh(&mut self) -> String {
        let name = format!("%t{}", self.temp);
        self.temp += 1;
        name
    }

    /// The instruction that raises the same signal Cranelift's own trap
    /// does on this target (`docs/llvm-backend.md` §3.2, §3.3): `ud2` on
    /// x86-64, measured in this slice's own session to assemble to
    /// Cranelift's exact two bytes and exit `SIGILL` every time; `udf
    /// #0xc11f` on aarch64, measured in the design doc's session the same
    /// way. Trap **codegen** is not generic between targets -- only the
    /// checker's decision to trap is (§3.2) -- so a target this match does
    /// not name is refused rather than guessed at.
    fn trap_asm(&self) -> Result<&'static str, String> {
        match self.triple.architecture {
            target_lexicon::Architecture::X86_64 => Ok("ud2"),
            target_lexicon::Architecture::Aarch64(_) => Ok("udf #0xc11f"),
            other => Err(format!(
                "the LLVM backend's checked arithmetic has no measured trap instruction for \
                 `{other}` -- only x86-64 and aarch64 are measured (docs/llvm-backend.md §3.2, §3.3)"
            )),
        }
    }

    /// Open a `trapN`/`okN` pair: the caller emits its overflow condition,
    /// branches here, fills `trapN` with the target's trap instruction
    /// (always `unreachable` afterwards -- a trap never returns), and
    /// keeps emitting into `okN`, which this function leaves open.
    fn trap_if(&mut self, condition: &str) -> Result<(), String> {
        let n = self.blocks;
        self.blocks += 1;
        let (trap, ok) = (format!("trap{n}"), format!("ok{n}"));
        self.out.push_str(&format!("  br i1 {condition}, label %{trap}, label %{ok}\n"));
        self.out.push_str(&format!("{trap}:\n"));
        self.out
            .push_str(&format!("  call void asm sideeffect \"{}\", \"\"()\n", self.trap_asm()?));
        self.out.push_str("  unreachable\n");
        self.out.push_str(&format!("{ok}:\n"));
        Ok(())
    }

    /// An expression whose value is exactly one leaf -- every `int`/`bool`
    /// operand a `BinOp` takes.
    fn scalar(&mut self, expr: &Expr) -> Result<LValue, String> {
        let mut values = self.expr(expr)?;
        if values.len() != 1 {
            return Err(format!(
                "expected a single-leaf `int`/`bool` value here, got {} leaves",
                values.len()
            ));
        }
        Ok(values.remove(0))
    }

    /// `docs/llvm-backend.md` §5: every `BinOp`. The two short-circuit
    /// logical operators are handled before either operand is evaluated
    /// -- `rhs` must not run when `lhs` already decided the answer, which
    /// is the one thing this function's usual "evaluate both, then
    /// dispatch" shape must not do here.
    fn binop(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<Vec<LValue>, String> {
        if op.is_short_circuit() {
            return self.short_circuit(op, lhs, rhs);
        }
        let a = self.scalar(lhs)?;
        let b = self.scalar(rhs)?;
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

    /// `Add`/`Sub`/`Mul`: LLVM's own overflow-reporting intrinsics, the
    /// direct counterpart of Cranelift's `sadd_overflow`/`ssub_overflow`/
    /// `smul_overflow` -- checked against a real `clang` in this slice's
    /// own session (`docs/llvm-backend.md` §5).
    fn checked_arith(&mut self, op: &str, a: LValue, b: LValue) -> Result<Vec<LValue>, String> {
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
    fn checked_div(
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
    fn checked_shift(&mut self, op: BinOp, a: LValue, b: LValue) -> Result<Vec<LValue>, String> {
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
    fn plain(&mut self, instr: &str, a: LValue, b: LValue) -> LValue {
        let result = self.fresh();
        self.out.push_str(&format!("  {result} = {instr} i64 {}, {}\n", operand(&a), operand(&b)));
        LValue::Reg(result)
    }

    /// The six comparisons. `icmp` yields `i1`; `zext`ed to `i8` because
    /// that is this backend's `bool` leaf, the same widening Cranelift's
    /// `icmp` needs none of (its `i8` result already is one).
    fn compare(&mut self, op: BinOp, a: LValue, b: LValue) -> LValue {
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
    fn truthy(&mut self, value: &LValue) -> String {
        let cond = self.fresh();
        self.out.push_str(&format!("  {cond} = icmp ne i8 {}, 0\n", operand(value)));
        cond
    }

    /// `&&`/`||`: `ir.rs`'s own comment says these lower as control flow
    /// rather than as an instruction, because `rhs` must not run once
    /// `lhs` has already decided the answer. A one-leaf `alloca` holds
    /// the result across the two blocks that can write it -- the same
    /// "memory instead of a phi" choice every other branch in this file
    /// makes -- so this needs no merge instruction, only two stores and a
    /// load.
    fn short_circuit(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<Vec<LValue>, String> {
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

    fn slot_reg(slot: u32, leaf: u32) -> String {
        format!("%s{slot}_{leaf}")
    }

    fn emit(&mut self) -> Result<String, String> {
        let ret_kinds = leaves_of(&self.func.ret, self.program)?;
        if ret_kinds.len() > 1 {
            return Err(format!(
                "`{}` returns more than one leaf, which the LLVM backend's first slice cannot \
                 hand back yet (indirect returns are not implemented)",
                self.func.name
            ));
        }
        let ret_ty = match ret_kinds.first() {
            Some(k) => k.llvm(),
            None => "void",
        };

        let mut params = Vec::new();
        let mut param_index = 0u32;
        for slot in 0..self.func.n_params {
            for (leaf, kind) in self.slot_kinds[slot as usize].clone().into_iter().enumerate() {
                params.push(format!("{} %arg{param_index}", kind.llvm()));
                let _ = leaf;
                param_index += 1;
            }
        }

        self.out.push_str(&format!(
            "define {ret_ty} @lexs_{}({}) {{\n",
            self.func.name,
            params.join(", ")
        ));
        self.out.push_str("entry:\n");

        // Every leaf of every slot gets its own `alloca`, parameters and
        // locals alike -- the memory `Stmt::Store`/`Expr::Load` read and
        // write, promoted to registers by `clang`'s own `mem2reg` rather
        // than by anything built here.
        for (slot, kinds) in self.slot_kinds.clone().iter().enumerate() {
            for (leaf, kind) in kinds.iter().enumerate() {
                self.out.push_str(&format!(
                    "  {} = alloca {}\n",
                    Self::slot_reg(slot as u32, leaf as u32),
                    kind.llvm()
                ));
            }
        }

        let mut arg_index = 0u32;
        for slot in 0..self.func.n_params {
            for (leaf, kind) in self.slot_kinds[slot as usize].clone().into_iter().enumerate() {
                self.out.push_str(&format!(
                    "  store {} %arg{arg_index}, ptr {}\n",
                    kind.llvm(),
                    Self::slot_reg(slot, leaf as u32)
                ));
                arg_index += 1;
            }
        }
        for slot in self.func.n_params..self.func.n_slots() {
            for (leaf, kind) in self.slot_kinds[slot as usize].clone().into_iter().enumerate() {
                self.out.push_str(&format!(
                    "  store {} {}, ptr {}\n",
                    kind.llvm(),
                    kind.zero(),
                    Self::slot_reg(slot, leaf as u32)
                ));
            }
        }

        let body = self.func.body.clone();
        let terminated = self.stmts(&body)?;
        if !terminated {
            match ret_kinds.first() {
                Some(k) => self.out.push_str(&format!("  ret {} {}\n", k.llvm(), k.zero())),
                None => self.out.push_str("  ret void\n"),
            }
        }
        self.out.push_str("}\n");
        Ok(std::mem::take(&mut self.out))
    }

    /// Emit a statement list; returns whether control left via `return`.
    fn stmts(&mut self, stmts: &[Stmt]) -> Result<bool, String> {
        for stmt in stmts {
            match stmt {
                Stmt::Store { place: Place::Slot(slot), value } => {
                    let values = self.expr(value)?;
                    let kinds = self.slot_kinds[slot.0 as usize].clone();
                    for (leaf, (kind, value)) in kinds.iter().zip(values).enumerate() {
                        self.out.push_str(&format!(
                            "  store {} {}, ptr {}\n",
                            kind.llvm(),
                            operand(&value),
                            Self::slot_reg(slot.0, leaf as u32)
                        ));
                    }
                }
                Stmt::Store { .. } => {
                    return Err(
                        "only a whole local is an assignable place in the LLVM backend's first \
                         slice (docs/llvm-backend.md §5)"
                            .to_owned(),
                    );
                }
                Stmt::Eval(expr) => {
                    self.expr(expr)?;
                }
                Stmt::Return(expr) => {
                    let values = self.expr(expr)?;
                    match values.len() {
                        0 => self.out.push_str("  ret void\n"),
                        1 => {
                            let kinds = leaves_of(&self.func.ret, self.program)?;
                            self.out.push_str(&format!(
                                "  ret {} {}\n",
                                kinds[0].llvm(),
                                operand(&values[0])
                            ));
                        }
                        _ => return Err("a multi-leaf return is not implemented yet".to_owned()),
                    }
                    return Ok(true);
                }
                Stmt::Borrow { referent, reference, unique, body } => {
                    if self.borrow_stmt(*referent, *reference, *unique, body)? {
                        return Ok(true);
                    }
                }
                Stmt::If { cond, then_body, else_body } => {
                    if self.if_stmt(cond, then_body, else_body)? {
                        return Ok(true);
                    }
                }
                Stmt::While { cond, body } => self.while_stmt(cond, body)?,
                Stmt::Match { .. } => {
                    return Err(
                        "`match` is not part of the LLVM backend yet -- it needs the enum \
                         layout this backend does not lower (docs/llvm-backend.md §5); \
                         `--backend cranelift` builds this program"
                            .to_owned(),
                    );
                }
                Stmt::Region { .. } => {
                    return Err(
                        "`region` is not part of the LLVM backend yet -- it needs the arena \
                         allocation this backend does not lower (docs/llvm-backend.md §5); \
                         `--backend cranelift` builds this program"
                            .to_owned(),
                    );
                }
            }
        }
        Ok(false)
    }

    fn borrow_stmt(
        &mut self,
        referent: Slot,
        reference: Slot,
        unique: bool,
        body: &[Stmt],
    ) -> Result<bool, String> {
        let kinds = self.slot_kinds[referent.0 as usize].clone();
        // One `i8` buffer per leaf-width-of-8-bytes, `.max(1)` so a
        // zero-leaf referent (every capability) still gets a real address
        // to hand the reference -- never dereferenced, since nothing this
        // slice lowers reads through a reference's pointee.
        let bytes = (kinds.len() as u32 * 8).max(1);
        let buffer = self.fresh();
        self.out.push_str(&format!("  {buffer} = alloca i8, i64 {bytes}\n"));

        let mut loaded = Vec::with_capacity(kinds.len());
        for (leaf, kind) in kinds.iter().enumerate() {
            let reg = self.fresh();
            self.out.push_str(&format!(
                "  {reg} = load {}, ptr {}\n",
                kind.llvm(),
                Self::slot_reg(referent.0, leaf as u32)
            ));
            loaded.push(LValue::Reg(reg));
        }
        self.store_leaves(&buffer, &kinds, &loaded);

        // The reference is one pointer leaf, always -- `abi::leaves_into`'s
        // rule for `Type::Ref` -- pointing at the buffer just filled.
        self.out
            .push_str(&format!("  store ptr {buffer}, ptr {}\n", Self::slot_reg(reference.0, 0)));

        let terminated = self.stmts(body)?;

        if unique && !terminated {
            let restored = self.load_leaves(&buffer, &kinds);
            for (leaf, value) in restored.into_iter().enumerate() {
                self.out.push_str(&format!(
                    "  store {} {}, ptr {}\n",
                    kinds[leaf].llvm(),
                    operand(&value),
                    Self::slot_reg(referent.0, leaf as u32)
                ));
            }
        }
        Ok(terminated)
    }

    /// `if`/`else`. Returns whether *both* arms terminate -- the checker's
    /// own `terminates()` rule, and the reason a terminating `if` is
    /// always a block's last statement: nothing here needs to merge a
    /// live value between the two arms, because every local this backend
    /// has is memory (`docs/llvm-backend.md` §5's own note on why this
    /// crate never builds a `phi`), so the block after the `if` simply
    /// reads whatever the taken arm last wrote.
    fn if_stmt(
        &mut self,
        cond: &Expr,
        then_body: &[Stmt],
        else_body: &[Stmt],
    ) -> Result<bool, String> {
        let value = self.scalar(cond)?;
        let cond1 = self.truthy(&value);
        let n = self.blocks;
        self.blocks += 1;
        let (then_label, else_label, end_label) =
            (format!("then{n}"), format!("else{n}"), format!("endif{n}"));
        self.out.push_str(&format!("  br i1 {cond1}, label %{then_label}, label %{else_label}\n"));

        self.out.push_str(&format!("{then_label}:\n"));
        let then_terminated = self.stmts(then_body)?;
        if !then_terminated {
            self.out.push_str(&format!("  br label %{end_label}\n"));
        }

        self.out.push_str(&format!("{else_label}:\n"));
        let else_terminated = self.stmts(else_body)?;
        if !else_terminated {
            self.out.push_str(&format!("  br label %{end_label}\n"));
        }

        let terminated = then_terminated && else_terminated;
        // A block with no predecessor left (both arms terminated) is
        // never branched to, and LLVM still requires it to end in a
        // terminator if it exists at all -- so it is simplest not to
        // emit it: nothing after this `if` runs either way, the same
        // fact `terminates()` already established for the caller.
        if !terminated {
            self.out.push_str(&format!("{end_label}:\n"));
        }
        Ok(terminated)
    }

    /// `while`. Never itself a terminator -- `terminates()`'s own rule,
    /// because a `while` might run zero times -- so this has no bool to
    /// return, unlike `if_stmt`/`borrow_stmt`. The loop header is
    /// re-entered by the back edge as well as by the first fall-through,
    /// but it needs no `phi` for the same reason `if_stmt` needs none:
    /// `cond` reads current values out of memory fresh on every entry.
    fn while_stmt(&mut self, cond: &Expr, body: &[Stmt]) -> Result<(), String> {
        let n = self.blocks;
        self.blocks += 1;
        let (head, body_label, end_label) =
            (format!("loophead{n}"), format!("loopbody{n}"), format!("loopend{n}"));

        self.out.push_str(&format!("  br label %{head}\n"));
        self.out.push_str(&format!("{head}:\n"));
        let value = self.scalar(cond)?;
        let cond1 = self.truthy(&value);
        self.out.push_str(&format!("  br i1 {cond1}, label %{body_label}, label %{end_label}\n"));

        self.out.push_str(&format!("{body_label}:\n"));
        let terminated = self.stmts(body)?;
        if !terminated {
            self.out.push_str(&format!("  br label %{head}\n"));
        }

        self.out.push_str(&format!("{end_label}:\n"));
        Ok(())
    }

    fn store_leaves(&mut self, buffer: &str, kinds: &[LKind], values: &[LValue]) {
        for (leaf, (kind, value)) in kinds.iter().zip(values).enumerate() {
            let addr = self.fresh();
            self.out.push_str(&format!(
                "  {addr} = getelementptr i8, ptr {buffer}, i64 {}\n",
                leaf as i64 * 8
            ));
            self.out.push_str(&format!("  store {} {}, ptr {addr}\n", kind.llvm(), operand(value)));
        }
    }

    fn load_leaves(&mut self, buffer: &str, kinds: &[LKind]) -> Vec<LValue> {
        kinds
            .iter()
            .enumerate()
            .map(|(leaf, kind)| {
                let addr = self.fresh();
                self.out.push_str(&format!(
                    "  {addr} = getelementptr i8, ptr {buffer}, i64 {}\n",
                    leaf as i64 * 8
                ));
                let reg = self.fresh();
                self.out.push_str(&format!("  {reg} = load {}, ptr {addr}\n", kind.llvm()));
                LValue::Reg(reg)
            })
            .collect()
    }

    fn expr(&mut self, expr: &Expr) -> Result<Vec<LValue>, String> {
        match expr {
            Expr::Int(v) => Ok(vec![LValue::Const(*v)]),
            Expr::Bool(v) => Ok(vec![LValue::Const(i64::from(*v))]),
            Expr::Load(slot) => {
                let kinds = self.slot_kinds[slot.0 as usize].clone();
                let mut out = Vec::with_capacity(kinds.len());
                for (leaf, kind) in kinds.iter().enumerate() {
                    let reg = self.fresh();
                    self.out.push_str(&format!(
                        "  {reg} = load {}, ptr {}\n",
                        kind.llvm(),
                        Self::slot_reg(slot.0, leaf as u32)
                    ));
                    out.push(LValue::Reg(reg));
                }
                Ok(out)
            }
            Expr::Field { base, def, args, index } => {
                let values = self.expr(base)?;
                let lex_sys_ir::TypeInfo::Struct { fields, .. } = self.program.type_info(*def)
                else {
                    return Err("a field access on an enum is not part of this slice".to_owned());
                };
                let mut start = 0usize;
                for (_, ty) in &fields[..*index as usize] {
                    start += leaves_of(&ty.substitute(args, &[]), self.program)?.len();
                }
                let len =
                    leaves_of(&fields[*index as usize].1.substitute(args, &[]), self.program)?
                        .len();
                Ok(values[start..start + len].to_vec())
            }
            Expr::Call { callee, args } => self.call(callee, args),
            Expr::Bin { op, lhs, rhs } => self.binop(*op, lhs, rhs),
            other => Err(format!(
                "`{other:?}` is not part of the LLVM backend's first slice (docs/llvm-backend.md §5)"
            )),
        }
    }

    fn call(&mut self, callee: &Callee, args: &[Expr]) -> Result<Vec<LValue>, String> {
        let evaluated: Vec<Vec<LValue>> =
            args.iter().map(|a| self.expr(a)).collect::<Result<_, _>>()?;

        match callee {
            Callee::Builtin(Builtin::Split | Builtin::Narrow) => Ok(Vec::new()),
            Callee::Builtin(Builtin::Release) => Ok(vec![LValue::Const(0)]),
            Callee::Builtin(Builtin::PutChar) => {
                let skip = Builtin::PutChar.erased_args();
                let c = evaluated
                    .into_iter()
                    .skip(skip)
                    .flatten()
                    .next()
                    .ok_or_else(|| "`putchar` needs a character argument".to_owned())?;
                let narrowed = self.fresh();
                self.out.push_str(&format!("  {narrowed} = trunc i64 {} to i32\n", operand(&c)));
                let result = self.fresh();
                self.out.push_str(&format!("  {result} = call i32 @putchar(i32 {narrowed})\n"));
                let widened = self.fresh();
                self.out.push_str(&format!("  {widened} = sext i32 {result} to i64\n"));
                Ok(vec![LValue::Reg(widened)])
            }
            Callee::Fn(id) => {
                let target = self.program.func(*id);
                let param_kinds: Vec<LKind> = target.slots[..target.n_params as usize]
                    .iter()
                    .map(|ty| leaves_of(ty, self.program))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .flatten()
                    .collect();
                let flat: Vec<LValue> = evaluated.into_iter().flatten().collect();
                if flat.len() != param_kinds.len() {
                    return Err(format!(
                        "`{}` takes {} leaves but {} were given",
                        target.name,
                        param_kinds.len(),
                        flat.len()
                    ));
                }
                let printed: Vec<String> = param_kinds
                    .iter()
                    .zip(&flat)
                    .map(|(kind, value)| format!("{} {}", kind.llvm(), operand(value)))
                    .collect();
                let ret_kinds = leaves_of(&target.ret, self.program)?;
                if ret_kinds.len() > 1 {
                    return Err(format!(
                        "`{}` returns more than one leaf, which the LLVM backend's first slice \
                         cannot call yet",
                        target.name
                    ));
                }
                match ret_kinds.first() {
                    None => {
                        self.out.push_str(&format!(
                            "  call void @lexs_{}({})\n",
                            target.name,
                            printed.join(", ")
                        ));
                        Ok(Vec::new())
                    }
                    Some(kind) => {
                        let result = self.fresh();
                        self.out.push_str(&format!(
                            "  {result} = call {} @lexs_{}({})\n",
                            kind.llvm(),
                            target.name,
                            printed.join(", ")
                        ));
                        Ok(vec![LValue::Reg(result)])
                    }
                }
            }
            Callee::Builtin(other) => Err(format!(
                "`{}` is not part of the LLVM backend's first slice (docs/llvm-backend.md §5)",
                other.name()
            )),
            Callee::Extern(_) => {
                Err("a foreign call is not part of the LLVM backend's first slice \
                 (docs/llvm-backend.md §5)"
                    .to_owned())
            }
        }
    }
}
