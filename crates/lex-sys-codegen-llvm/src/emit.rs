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
//!
//! `mem2reg` is not automatic: it runs as part of `clang`'s standard `-O1+`
//! pipeline, not at the default `-O0` (`docs/llvm-backend.md` §7 measured
//! the difference and corrected the earlier "mandatory" claim). `lib.rs`'s
//! `run_clang` always passes `-O2` for exactly this reason -- without it,
//! every leaf here stays a real stack slot.

use lex_sys_ir::{Arm, BinOp, Builtin, Callee, Expr, Func, Place, Program, Slot, Stmt};
use lex_sys_types::{DefId, Type};
use target_lexicon::Triple;

/// One arena's chunk, matching `lex-sys-codegen`'s own `abi::ARENA_CHUNK`
/// exactly (§7.5): some `benches/` programs (`sieve_checked.ls`'s own
/// header) size their allocation against this constant, so a mismatched
/// chunk size would trap where the Cranelift build does not, or the
/// reverse.
const ARENA_CHUNK: i64 = 64 * 1024;

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

/// The LLVM type a multi-leaf value returns as: an anonymous struct, one
/// field per leaf -- the same shape `checked_arith`'s own `{i64, i1}`
/// already is for LLVM's overflow intrinsics, applied here to a
/// `lex-sys` value with more than one leaf (`docs/llvm-backend.md`
/// §7.7: `contents(b)`'s own two leaves, called through a function like
/// `reduce_checked.ls`'s `fill`, is what this exists for).
fn struct_ty(kinds: &[LKind]) -> String {
    format!("{{{}}}", kinds.iter().map(|k| k.llvm()).collect::<Vec<_>>().join(", "))
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
        // `docs/heap.md` §3: a box at run time is a pointer and nothing
        // else -- no header, no refcount, no tag -- except a box of an
        // *unsized* referent, which carries the length too, because
        // nothing else knows how many elements there are (`docs/boxed-
        // slices.md` §2, the same pair `&r [T]` already is). Matches
        // `lex-sys-codegen`'s own `abi::leaves_into` exactly; `Box` is a
        // prelude type, not an ordinary struct `program.type_info` would
        // scalarise correctly on its own.
        Type::Named(def, args) if def.0 as usize == lex_sys_ir::PRELUDE_BOX => {
            out.push(LKind::Ptr);
            if matches!(args.first(), Some(Type::Slice(_))) {
                out.push(LKind::I64);
            }
        }
        Type::Named(def, args) => match program.type_info(*def) {
            lex_sys_ir::TypeInfo::Struct { fields, .. } => {
                for (_, field) in fields {
                    leaves_into(&field.substitute(args, &[]), program, out)?;
                }
            }
            // A tag, then *every* variant's payload leaves -- wasteful and
            // deliberately so, matching `lex-sys-codegen`'s own rule
            // (`abi::leaves_into`): overlaying the payloads is a layout
            // decision no M1 backend makes. The tag is an `i64` for the
            // same reason -- picking a narrower integer would be one too.
            lex_sys_ir::TypeInfo::Enum { variants, .. } => {
                out.push(LKind::I64);
                for (_, payload) in variants {
                    for ty in payload {
                        leaves_into(&ty.substitute(args, &[]), program, out)?;
                    }
                }
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
    // `region`/`alloc_slice` (§7.5): one `malloc` per arena, one `free` on
    // the way out -- `lex-sys-codegen`'s own `body/memory.rs` `libc_fn`
    // declares these the same way, on first use rather than unconditionally
    // there, but an unused `declare` here costs nothing, the same reasoning
    // `putchar`'s own unconditional declaration already relies on.
    text.push_str("declare ptr @malloc(i64)\n");
    text.push_str("declare void @free(ptr)\n");
    // Checked arithmetic (§5's second slice): the three overflow-reporting
    // intrinsics `Expr::Bin`'s `Add`/`Sub`/`Mul` arms call. Declared
    // unconditionally, the same way `putchar` is -- an unused `declare`
    // costs nothing, and every function in the module shares one `.ll`.
    text.push_str("declare {i64, i1} @llvm.sadd.with.overflow.i64(i64, i64)\n");
    text.push_str("declare {i64, i1} @llvm.ssub.with.overflow.i64(i64, i64)\n");
    text.push_str("declare {i64, i1} @llvm.smul.with.overflow.i64(i64, i64)\n\n");

    // Every string literal's bytes (§5's fourth slice) become one global
    // constant, named as it is met rather than once per unique text --
    // `docs/strings.md` §8 leaves interning an open question, so two
    // occurrences of the same literal get two objects here exactly as
    // `lex-sys-codegen`'s own `literals` counter gives them two. Built up
    // across every function before any of it is written into `text`,
    // because a function later in `program.funcs` may be the first one a
    // reader meets textually if `program.funcs` and source order ever
    // diverge (`docs/README.md`'s own "definition order never matters").
    let mut globals = String::new();
    let mut next_literal: u32 = 0;
    let mut bodies: Vec<String> = Vec::with_capacity(program.funcs.len());
    for (index, func) in program.funcs.iter().enumerate() {
        let body = FuncEmitter::new(program, func, triple, &mut globals, &mut next_literal)
            .and_then(|mut fe| fe.emit())
            .map_err(|message| (Some(index), message))?;
        bodies.push(body);
    }
    text.push_str(&globals);
    if !globals.is_empty() {
        text.push('\n');
    }
    for body in bodies {
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
    /// Module-level constant declarations, shared across every function's
    /// `FuncEmitter` (§5's fourth slice): a string literal's global lives
    /// here, not in `out`, because a global declaration is not valid
    /// inside a function body.
    globals: &'a mut String,
    /// Shared across every function, so two literals in two different
    /// functions still get two distinct symbol names.
    next_literal: &'a mut u32,
    out: String,
    temp: u32,
    /// Numbers each `trap`/`ok` block pair a checked operator opens
    /// (§5's second slice) -- distinct from `temp`, which numbers SSA
    /// registers, because a block label and a register share no namespace
    /// in LLVM IR but reusing one counter for both would still be correct;
    /// two counters just read clearer in the emitted text.
    blocks: u32,
    /// Per open arena, the two `ptr`-typed `alloca` cells holding its base
    /// and its bump pointer (§7.5) -- `lex-sys-codegen`'s own `arenas:
    /// Vec<Option<(Variable, Variable)>>` (`body/memory.rs`), the same
    /// shape read through this backend's own memory-not-`Variable` idiom.
    /// Indexed by arena number, which `lex-sys-ir` assigns positionally
    /// (`docs/llvm-backend.md` §7.5), so a sibling region after an earlier
    /// one closed may need the vector grown rather than pushed to.
    arenas: Vec<Option<(String, String)>>,
}

impl<'a> FuncEmitter<'a> {
    fn new(
        program: &'a Program,
        func: &'a Func,
        triple: &'a Triple,
        globals: &'a mut String,
        next_literal: &'a mut u32,
    ) -> Result<Self, String> {
        let slot_kinds =
            func.slots.iter().map(|ty| leaves_of(ty, program)).collect::<Result<Vec<_>, _>>()?;
        Ok(FuncEmitter {
            program,
            func,
            slot_kinds,
            triple,
            globals,
            next_literal,
            out: String::new(),
            temp: 0,
            blocks: 0,
            arenas: Vec::new(),
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

    /// `wrapping_add`/`sub`/`mul`: two `int` leaves in, `plain` handles the
    /// rest -- `instr` is `"add"`/`"sub"`/`"mul"`, which doubles as the
    /// builtin's own name suffix for the error message.
    fn wrapping(
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

    /// A zero of the function's own return type, for a block the checker
    /// proved unreachable but LLVM still requires well-formed -- the
    /// function's own fall-through at the end of `emit`, and `match_stmt`'s
    /// impossible tag-chain fall-through, need exactly the same thing.
    fn emit_default_return(&mut self) -> Result<(), String> {
        let ret_kinds = leaves_of(&self.func.ret, self.program)?;
        match ret_kinds.as_slice() {
            [] => self.out.push_str("  ret void\n"),
            [k] => self.out.push_str(&format!("  ret {} {}\n", k.llvm(), k.zero())),
            kinds => {
                let zeros: Vec<LValue> =
                    kinds.iter().map(|k| LValue::Reg(k.zero().to_owned())).collect();
                let agg = self.pack_struct(kinds, &zeros);
                self.out.push_str(&format!("  ret {} {agg}\n", struct_ty(kinds)));
            }
        }
        Ok(())
    }

    /// Pack `values` into one aggregate register, one field per leaf --
    /// the `insertvalue` chain a multi-leaf return needs, starting from
    /// `undef` the same way LLVM's own multi-result intrinsics
    /// (`checked_arith`'s `{i64, i1}`) are read back out of, in reverse.
    fn pack_struct(&mut self, kinds: &[LKind], values: &[LValue]) -> String {
        let ty = struct_ty(kinds);
        let mut agg = "undef".to_owned();
        for (i, (kind, value)) in kinds.iter().zip(values).enumerate() {
            let next = self.fresh();
            self.out.push_str(&format!(
                "  {next} = insertvalue {ty} {agg}, {} {}, {i}\n",
                kind.llvm(),
                operand(value)
            ));
            agg = next;
        }
        agg
    }

    fn emit(&mut self) -> Result<String, String> {
        let ret_kinds = leaves_of(&self.func.ret, self.program)?;
        let ret_ty = match ret_kinds.as_slice() {
            [] => "void".to_owned(),
            [k] => k.llvm().to_owned(),
            kinds => struct_ty(kinds),
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
            self.emit_default_return()?;
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
                Stmt::Store { place: Place::Element { base, index, element }, value } => {
                    let values = self.expr(value)?;
                    let addr = self.element_address(base, index, element)?;
                    let kinds = leaves_of(element, self.program)?;
                    self.store_leaves(&addr, &kinds, &values);
                }
                Stmt::Store { .. } => {
                    return Err(
                        "a `Field`/`Deref` place is not part of the LLVM backend yet -- both \
                         need the struct/box layout this backend does not lower \
                         (docs/llvm-backend.md §5)"
                            .to_owned(),
                    );
                }
                Stmt::Eval(expr) => {
                    self.expr(expr)?;
                }
                Stmt::Return(expr) => {
                    let values = self.expr(expr)?;
                    match values.as_slice() {
                        [] => self.out.push_str("  ret void\n"),
                        [value] => {
                            let kinds = leaves_of(&self.func.ret, self.program)?;
                            self.out.push_str(&format!(
                                "  ret {} {}\n",
                                kinds[0].llvm(),
                                operand(value)
                            ));
                        }
                        values => {
                            let kinds = leaves_of(&self.func.ret, self.program)?;
                            let agg = self.pack_struct(&kinds, values);
                            self.out.push_str(&format!("  ret {} {agg}\n", struct_ty(&kinds)));
                        }
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
                Stmt::Match { scrutinee, def, args, arms, by_reference } => {
                    if self.match_stmt(scrutinee, *def, args, arms, *by_reference)? {
                        return Ok(true);
                    }
                }
                Stmt::Region { arena, body } => {
                    if self.region_stmt(*arena, body)? {
                        return Ok(true);
                    }
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

    /// `region a { .. }` -- one `malloc` in, one `free` out (§7.5), the
    /// same shape `lex-sys-codegen`'s own `region_stmt` has
    /// (`body/memory.rs`): between them the arena is two pointers, where
    /// the next allocation goes and where the chunk ends, and the end is
    /// never stored because it is the base plus `ARENA_CHUNK`, a constant.
    fn region_stmt(&mut self, arena: u32, body: &[Stmt]) -> Result<bool, String> {
        let base = self.fresh();
        self.out.push_str(&format!("  {base} = call ptr @malloc(i64 {ARENA_CHUNK})\n"));
        // Out of memory is a trap, not a null pointer wandering into a
        // store -- the language has no undefined behaviour to fall back
        // on, the same reasoning `box`/`box_slice` already trap on here.
        let is_null = self.fresh();
        self.out.push_str(&format!("  {is_null} = icmp eq ptr {base}, null\n"));
        self.trap_if(&is_null)?;

        let base_cell = self.fresh();
        self.out.push_str(&format!("  {base_cell} = alloca ptr\n"));
        self.out.push_str(&format!("  store ptr {base}, ptr {base_cell}\n"));
        let bump_cell = self.fresh();
        self.out.push_str(&format!("  {bump_cell} = alloca ptr\n"));
        self.out.push_str(&format!("  store ptr {base}, ptr {bump_cell}\n"));

        // Grow to fit rather than push: a sibling `region` carries a
        // higher number than one already closed (`lex-sys-ir` assigns
        // arena numbers positionally), so this slot may be past the
        // vector's current end.
        if self.arenas.len() <= arena as usize {
            self.arenas.resize(arena as usize + 1, None);
        }
        self.arenas[arena as usize] = Some((base_cell.clone(), bump_cell));

        let terminated = self.stmts(body)?;

        // Skipped when the body returned: the `Stmt::Return` arm above
        // this arena's `free` would need has already run, and there is
        // no block left here to put a second call in.
        if !terminated {
            let held = self.fresh();
            self.out.push_str(&format!("  {held} = load ptr, ptr {base_cell}\n"));
            self.out.push_str(&format!("  call void @free(ptr {held})\n"));
        }
        self.arenas[arena as usize] = None;
        Ok(terminated)
    }

    /// Take `bytes` from an arena, trapping if the chunk cannot spare
    /// them -- `lex-sys-codegen`'s own `bump` (`body/memory.rs`), kept in
    /// `ptr` arithmetic throughout rather than round-tripping through
    /// `ptrtoint`: `getelementptr`/`icmp` both work directly on `ptr`.
    fn bump(&mut self, arena: u32, bytes: &LValue) -> Result<LValue, String> {
        let (base_cell, bump_cell) = self
            .arenas
            .get(arena as usize)
            .cloned()
            .flatten()
            .ok_or_else(|| format!("arena {arena} is not open here"))?;

        let at = self.fresh();
        self.out.push_str(&format!("  {at} = load ptr, ptr {bump_cell}\n"));
        let next = self.fresh();
        self.out
            .push_str(&format!("  {next} = getelementptr i8, ptr {at}, i64 {}\n", operand(bytes)));

        let base = self.fresh();
        self.out.push_str(&format!("  {base} = load ptr, ptr {base_cell}\n"));
        let end = self.fresh();
        self.out.push_str(&format!("  {end} = getelementptr i8, ptr {base}, i64 {ARENA_CHUNK}\n"));

        // Two ways to be past the end, and a slice can hit either: the sum
        // overshoots the chunk, or the size computation itself wrapped and
        // the sum came out *before* where it started -- the same two
        // comparisons `lex-sys-codegen`'s own `bump` makes.
        let over = self.fresh();
        self.out.push_str(&format!("  {over} = icmp ugt ptr {next}, {end}\n"));
        let wrapped = self.fresh();
        self.out.push_str(&format!("  {wrapped} = icmp ult ptr {next}, {at}\n"));
        let bad = self.fresh();
        self.out.push_str(&format!("  {bad} = or i1 {over}, {wrapped}\n"));
        self.trap_if(&bad)?;

        self.out.push_str(&format!("  store ptr {next}, ptr {bump_cell}\n"));
        Ok(LValue::Reg(at))
    }

    /// Write `values` into every element of a freshly reserved run -- a
    /// real loop, not unrolled, because `count` is a runtime value, the
    /// same reason `lex-sys-codegen`'s own `fill_slice` is one
    /// (`body/memory.rs`). `values` are computed once by the caller and
    /// written unchanged into every element, matching that function
    /// exactly.
    fn fill_slice(
        &mut self,
        start: &LValue,
        count: &LValue,
        stride: i64,
        kinds: &[LKind],
        values: &[LValue],
    ) -> Result<(), String> {
        let n = self.blocks;
        self.blocks += 1;
        let (head, body_label, done) =
            (format!("fillhead{n}"), format!("fillbody{n}"), format!("filldone{n}"));

        let cursor = self.fresh();
        self.out.push_str(&format!("  {cursor} = alloca i64\n"));
        self.out.push_str(&format!("  store i64 0, ptr {cursor}\n"));
        self.out.push_str(&format!("  br label %{head}\n"));

        self.out.push_str(&format!("{head}:\n"));
        let i = self.fresh();
        self.out.push_str(&format!("  {i} = load i64, ptr {cursor}\n"));
        let more = self.fresh();
        self.out.push_str(&format!("  {more} = icmp slt i64 {i}, {}\n", operand(count)));
        self.out.push_str(&format!("  br i1 {more}, label %{body_label}, label %{done}\n"));

        self.out.push_str(&format!("{body_label}:\n"));
        let offset = self.fresh();
        self.out.push_str(&format!("  {offset} = mul i64 {i}, {stride}\n"));
        let addr = self.fresh();
        self.out.push_str(&format!(
            "  {addr} = getelementptr i8, ptr {}, i64 {offset}\n",
            operand(start)
        ));
        self.store_leaves(&addr, kinds, values);
        let next = self.fresh();
        self.out.push_str(&format!("  {next} = add i64 {i}, 1\n"));
        self.out.push_str(&format!("  store i64 {next}, ptr {cursor}\n"));
        self.out.push_str(&format!("  br label %{head}\n"));

        self.out.push_str(&format!("{done}:\n"));
        Ok(())
    }

    /// How many bytes `count` elements of stride `stride` take, checked --
    /// `lex-sys-codegen`'s own `slice_bytes` (`body/memory.rs`), shared
    /// here between `alloc_slice` and `boxed_slice` the same way. A
    /// negative count traps ahead of the multiply, and the multiply
    /// itself is `checked_arith`'s own `smul` -- the intrinsic
    /// `lex-sys-codegen`'s version reaches for by a different name
    /// (`smul_overflow`) for the identical reason: a length that
    /// overflows the byte count would ask for less memory than is about
    /// to be written.
    fn slice_bytes(&mut self, count: &LValue, stride: i64) -> Result<LValue, String> {
        let negative = self.fresh();
        self.out.push_str(&format!("  {negative} = icmp slt i64 {}, 0\n", operand(count)));
        self.trap_if(&negative)?;

        let bytes = self.checked_arith("smul", count.clone(), LValue::Const(stride))?;
        Ok(bytes.into_iter().next().expect("`checked_arith` returns exactly one value"))
    }

    /// `alloc_slice[a](count, fill)` -- `count` copies of `fill`,
    /// contiguous (`lex-sys-codegen`'s own `alloc_slice`, `body/
    /// memory.rs`).
    fn alloc_slice(
        &mut self,
        arena: u32,
        element: &Type,
        count: &Expr,
        fill: &Expr,
    ) -> Result<Vec<LValue>, String> {
        let count = self.scalar(count)?;
        let values = self.expr(fill)?;
        let stride = self.stride_of(element)?;
        let bytes = self.slice_bytes(&count, stride)?;

        let start = self.bump(arena, &bytes)?;
        let kinds = leaves_of(element, self.program)?;
        self.fill_slice(&start, &count, stride, &kinds, &values)?;
        Ok(vec![start, count])
    }

    /// `box_slice(h, count, fill)` (`docs/boxed-slices.md` §3, §7.7): the
    /// same sizing and the same fill `alloc_slice` uses; only where the
    /// memory comes from differs -- one `malloc`, trapping on exhaustion
    /// exactly as `region_stmt`'s own arena chunk does, rather than a
    /// bump within one. What comes back is two leaves, a pointer *and* a
    /// length, because nothing else knows how many elements there are
    /// (`lex-sys-codegen`'s own `boxed_slice`, `body/memory.rs`).
    fn boxed_slice(
        &mut self,
        element: &Type,
        count: &Expr,
        fill: &Expr,
    ) -> Result<Vec<LValue>, String> {
        let count = self.scalar(count)?;
        let values = self.expr(fill)?;
        let stride = self.stride_of(element)?;
        let bytes = self.slice_bytes(&count, stride)?;

        let start = self.fresh();
        self.out.push_str(&format!("  {start} = call ptr @malloc(i64 {})\n", operand(&bytes)));
        let is_null = self.fresh();
        self.out.push_str(&format!("  {is_null} = icmp eq ptr {start}, null\n"));
        self.trap_if(&is_null)?;

        let kinds = leaves_of(element, self.program)?;
        let start = LValue::Reg(start);
        self.fill_slice(&start, &count, stride, &kinds, &values)?;
        Ok(vec![start, count])
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

    /// Where one variant's payload starts among the whole enum's leaves,
    /// and how many leaves each of its payload fields is -- matching
    /// `lex-sys-codegen`'s own `variant_layout` exactly: one leaf for the
    /// tag, then every *earlier* variant's payload, whether or not this
    /// value is ever that variant.
    fn variant_layout(
        &self,
        def: DefId,
        args: &[Type],
        variant: u32,
    ) -> Result<(usize, Vec<usize>), String> {
        let lex_sys_ir::TypeInfo::Enum { variants, .. } = self.program.type_info(def) else {
            return Err("a variant index on a type that is not an enum".to_owned());
        };
        let width = |ty: &Type| -> Result<usize, String> {
            Ok(leaves_of(&ty.substitute(args, &[]), self.program)?.len())
        };
        let mut offset = 1usize;
        for (_, payload) in &variants[..variant as usize] {
            for ty in payload {
                offset += width(ty)?;
            }
        }
        let widths =
            variants[variant as usize].1.iter().map(width).collect::<Result<Vec<_>, _>>()?;
        Ok((offset, widths))
    }

    /// `Shape::Circle(2)`: the tag, then every variant's leaves -- this
    /// variant's are the values just computed, the rest zeroed, because a
    /// value that is not this variant is not readable without matching on
    /// the tag first (matching `lex-sys-codegen`'s own `Expr::Enum` arm).
    fn enum_lit(
        &mut self,
        def: DefId,
        args: &[Type],
        variant: u32,
        payload: &[Expr],
    ) -> Result<Vec<LValue>, String> {
        let whole = Type::Named(def, args.to_vec());
        let all_kinds = leaves_of(&whole, self.program)?;
        let (offset, widths) = self.variant_layout(def, args, variant)?;
        let payload_values: Vec<Vec<LValue>> =
            payload.iter().map(|e| self.expr(e)).collect::<Result<_, _>>()?;

        let mut out: Vec<LValue> = Vec::with_capacity(all_kinds.len());
        out.push(LValue::Const(i64::from(variant)));
        for kind in &all_kinds[1..] {
            out.push(if *kind == LKind::Ptr {
                LValue::Reg("null".to_owned())
            } else {
                LValue::Const(0)
            });
        }
        let mut at = offset;
        for (values, width) in payload_values.into_iter().zip(widths) {
            debug_assert_eq!(values.len(), width);
            for value in values {
                out[at] = value;
                at += 1;
            }
        }
        Ok(out)
    }

    /// Copy a matched variant's payload into the slots its pattern bound
    /// -- the by-value half of `lex-sys-codegen`'s own `bind_payload`
    /// (matching *through* a reference is not part of this backend yet,
    /// so there is no address-only half to mirror here).
    fn bind_payload(
        &mut self,
        def: DefId,
        args: &[Type],
        variant: u32,
        arm: &Arm,
        values: &[LValue],
    ) -> Result<(), String> {
        let (offset, widths) = self.variant_layout(def, args, variant)?;
        let mut at = offset;
        for (binding, width) in arm.bindings.iter().zip(widths) {
            if let Some(slot) = binding {
                let kinds = self.slot_kinds[slot.0 as usize].clone();
                for index in 0..width {
                    let value = values[at + index].clone();
                    self.out.push_str(&format!(
                        "  store {} {}, ptr {}\n",
                        kinds[index].llvm(),
                        operand(&value),
                        Self::slot_reg(slot.0, index as u32)
                    ));
                }
            }
            // A `_` binding still occupies its payload position; there is
            // simply nowhere to put the value.
            at += width;
        }
        Ok(())
    }

    /// `match`, lowered as a chain of tag tests -- a jump table would be
    /// faster and is the obvious later move, matching
    /// `lex-sys-codegen`'s own reasoning exactly. Returns whether every
    /// arm returned, which makes the whole `match` a terminator.
    ///
    /// The merge block is always emitted, even when every tested arm
    /// returns: the checker proves the arms exhaustive, but a chain that
    /// falls all the way through without a wildcard still needs
    /// somewhere well-formed to land, the same "unreachable but must
    /// still be legal IR" reasoning `if_stmt` never needs (its two arms
    /// are syntactically exhaustive) and this backend's own function
    /// fall-through (`emit_default_return`) already has.
    fn match_stmt(
        &mut self,
        scrutinee: &Expr,
        def: DefId,
        args: &[Type],
        arms: &[Arm],
        by_reference: bool,
    ) -> Result<bool, String> {
        if by_reference {
            return Err("matching through a reference is not part of the LLVM backend yet \
                 (docs/llvm-backend.md §5); `--backend cranelift` builds this program"
                .to_owned());
        }
        let values = self.expr(scrutinee)?;
        let Some(tag) = values.first().cloned() else {
            return Err("a match scrutinee must be an enum (at least one leaf: the tag)".to_owned());
        };

        let n = self.blocks;
        self.blocks += 1;
        let merge = format!("matchend{n}");

        let mut all_returned = true;
        // Whether the fall-through chain still has an open block. A
        // wildcard arm closes it, because nothing can follow one.
        let mut open = true;

        for (arm_index, arm) in arms.iter().enumerate() {
            if !open {
                break;
            }
            match arm.variant {
                Some(variant) => {
                    let body_label = format!("matcharm{n}_{arm_index}");
                    let next_label = format!("matchnext{n}_{arm_index}");
                    let matched = self.fresh();
                    self.out.push_str(&format!(
                        "  {matched} = icmp eq i64 {}, {variant}\n",
                        operand(&tag)
                    ));
                    self.out.push_str(&format!(
                        "  br i1 {matched}, label %{body_label}, label %{next_label}\n"
                    ));

                    self.out.push_str(&format!("{body_label}:\n"));
                    self.bind_payload(def, args, variant, arm, &values)?;
                    let returned = self.stmts(&arm.body)?;
                    if !returned {
                        self.out.push_str(&format!("  br label %{merge}\n"));
                    }
                    all_returned &= returned;

                    self.out.push_str(&format!("{next_label}:\n"));
                }
                None => {
                    // The wildcard binds nothing and needs no test: it
                    // runs right here, in the block the chain fell
                    // through to.
                    let returned = self.stmts(&arm.body)?;
                    if !returned {
                        self.out.push_str(&format!("  br label %{merge}\n"));
                    }
                    all_returned &= returned;
                    open = false;
                }
            }
        }

        if open {
            // The checker proved the arms exhaustive, so this is
            // unreachable. It still needs filling: an unterminated block
            // is not legal IR.
            self.out.push_str(&format!("  br label %{merge}\n"));
        }

        self.out.push_str(&format!("{merge}:\n"));
        if all_returned {
            self.emit_default_return()?;
        }
        Ok(all_returned)
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
            // A struct value is positional already, in declaration order
            // -- the same shape `Expr::Tuple` would be -- so its leaves
            // are just every field's leaves, concatenated.
            Expr::Struct { fields, .. } => {
                let mut out = Vec::new();
                for field in fields {
                    out.extend(self.expr(field)?);
                }
                Ok(out)
            }
            Expr::Enum { def, args, variant, payload } => {
                self.enum_lit(*def, args, *variant, payload)
            }
            Expr::Call { callee, args } => self.call(callee, args),
            Expr::Bin { op, lhs, rhs } => self.binop(*op, lhs, rhs),
            Expr::Bytes(text) => Ok(self.bytes_lit(text)),
            // The length is the slice's second leaf -- already there,
            // never computed, exactly as `lex-sys-codegen`'s own
            // `Expr::Len` arm reads it.
            Expr::Len(slice) => {
                let mut values = self.expr(slice)?;
                if values.len() != 2 {
                    return Err(
                        "`len`'s argument is not a slice (expected 2 leaves: pointer and length)"
                            .to_owned(),
                    );
                }
                Ok(vec![values.remove(1)])
            }
            Expr::Index { base, index, element } => {
                let addr = self.element_address(base, index, element)?;
                let kinds = leaves_of(element, self.program)?;
                Ok(self.load_leaves(&addr, &kinds))
            }
            Expr::AllocSlice { arena, element, count, fill } => {
                self.alloc_slice(*arena, element, count, fill)
            }
            Expr::BoxedSlice { element, count, fill } => self.boxed_slice(element, count, fill),
            // One `free`, and the element count back -- the pointer is
            // the first leaf and the length the second (`docs/boxed-
            // slices.md` §2), the same order `boxed_slice` returns them.
            Expr::UnboxedSlice { value } => {
                let leaves = self.expr(value)?;
                if leaves.len() != 2 {
                    return Err(
                        "`unbox_slice`'s argument is not a boxed slice (expected 2 leaves: \
                         pointer and length)"
                            .to_owned(),
                    );
                }
                self.out.push_str(&format!("  call void @free(ptr {})\n", operand(&leaves[0])));
                Ok(vec![leaves[1].clone()])
            }
            // `contents(b)`: one load. A reference to a box points at
            // where the box's own leaves live, so reading them *is* the
            // reference to what the box holds -- a boxed slice's own two
            // leaves already *are* the `(pointer, length)` pair a plain
            // `[T]` is, which is why this doubles as `&r [T]` (`docs/
            // boxed-slices.md` §3). The second leaf, when present, sits
            // at the same 8-byte stride every leaf here does.
            Expr::Contents { ty, value } => {
                let reference = self.scalar(value)?;
                let held = self.fresh();
                self.out.push_str(&format!("  {held} = load ptr, ptr {}\n", operand(&reference)));
                if matches!(ty, Type::Slice(_)) {
                    let length_addr = self.fresh();
                    self.out.push_str(&format!(
                        "  {length_addr} = getelementptr i8, ptr {}, i64 8\n",
                        operand(&reference)
                    ));
                    let length = self.fresh();
                    self.out.push_str(&format!("  {length} = load i64, ptr {length_addr}\n"));
                    Ok(vec![LValue::Reg(held), LValue::Reg(length)])
                } else {
                    Ok(vec![LValue::Reg(held)])
                }
            }
            // `!b`: a `bool` leaf is 0 or 1, so flipping the low bit is
            // the negation -- `lex-sys-codegen`'s own `Expr::Not` arm
            // (`body/expr.rs`), one instruction and never trapping.
            Expr::Not(inner) => {
                let v = self.scalar(inner)?;
                let flipped = self.fresh();
                self.out.push_str(&format!("  {flipped} = xor i8 {}, 1\n", operand(&v)));
                Ok(vec![LValue::Reg(flipped)])
            }
            other => Err(format!(
                "`{other:?}` is not part of the LLVM backend yet (docs/llvm-backend.md §5)"
            )),
        }
    }

    /// A string literal's bytes (§5's fourth slice): one read-only global
    /// per occurrence, and no instruction needed to get its address --
    /// unlike Cranelift's `global_value`, an LLVM global symbol is
    /// already a usable `ptr` constant wherever one is expected. Written
    /// as a plain integer-array constant (`[i8 72, i8 105, ...]`) rather
    /// than the `c"..."` shorthand, which needs its own escaping rules
    /// this backend has no reason to also get right.
    fn bytes_lit(&mut self, text: &str) -> Vec<LValue> {
        let bytes = text.as_bytes();
        let name = format!("@str.{}", *self.next_literal);
        *self.next_literal += 1;
        let body = if bytes.is_empty() {
            "zeroinitializer".to_owned()
        } else {
            let items: Vec<String> = bytes.iter().map(|b| format!("i8 {b}")).collect();
            format!("[{}]", items.join(", "))
        };
        self.globals.push_str(&format!(
            "{name} = private unnamed_addr constant [{} x i8] {body}\n",
            bytes.len()
        ));
        vec![LValue::Reg(name), LValue::Const(bytes.len() as i64)]
    }

    /// The distance between elements of a `[T]`, matching
    /// `lex-sys-codegen`'s own `abi::stride_of`: a byte is the one size
    /// in the language not a multiple of 8 (`docs/strings.md` §3), so
    /// that a string is something C could read.
    fn stride_of(&self, element: &Type) -> Result<i64, String> {
        if matches!(element, Type::Byte) {
            Ok(1)
        } else {
            Ok(leaves_of(element, self.program)?.len() as i64 * 8)
        }
    }

    /// `s[i]` -- bounds-checked (`docs/defined-behaviour.md` §1), the
    /// address a read or a write both start from. `uge` catches a
    /// negative index the same way it already catches an out-of-range
    /// shift amount: reinterpreted as unsigned, a negative `i64` is far
    /// past any real length. The index-times-stride multiply is this
    /// backend's own address arithmetic, not user-level `*`, so it is
    /// plain `mul`, not `checked_arith`'s overflow-checked one -- the
    /// same distinction `lex-sys-codegen`'s address arithmetic draws.
    fn element_address(
        &mut self,
        base: &Expr,
        index: &Expr,
        element: &Type,
    ) -> Result<String, String> {
        let values = self.expr(base)?;
        if values.len() != 2 {
            return Err("indexing needs a slice (expected 2 leaves: pointer and length)".to_owned());
        }
        let (ptr, len) = (values[0].clone(), values[1].clone());
        let idx = self.scalar(index)?;
        let out_of_range = self.fresh();
        self.out.push_str(&format!(
            "  {out_of_range} = icmp uge i64 {}, {}\n",
            operand(&idx),
            operand(&len)
        ));
        self.trap_if(&out_of_range)?;
        let stride = self.stride_of(element)?;
        let offset = self.fresh();
        self.out.push_str(&format!("  {offset} = mul i64 {}, {stride}\n", operand(&idx)));
        let addr = self.fresh();
        self.out.push_str(&format!(
            "  {addr} = getelementptr i8, ptr {}, i64 {offset}\n",
            operand(&ptr)
        ));
        Ok(addr)
    }

    fn call(&mut self, callee: &Callee, args: &[Expr]) -> Result<Vec<LValue>, String> {
        let evaluated: Vec<Vec<LValue>> =
            args.iter().map(|a| self.expr(a)).collect::<Result<_, _>>()?;

        match callee {
            Callee::Builtin(Builtin::Split | Builtin::Narrow) => Ok(Vec::new()),
            Callee::Builtin(Builtin::Release) => Ok(vec![LValue::Const(0)]),
            // The escape from checked arithmetic (`docs/llvm-backend.md`
            // §7.3's first named gap): LLVM's own `add`/`sub`/`mul`, with
            // no `nsw`/`nuw` requested, are already two's-complement
            // wraparound -- `lex-sys-codegen`'s plain `iadd`/`isub`/`imul`
            // needs no overflow check either, so neither does this.
            Callee::Builtin(Builtin::WrappingAdd) => self.wrapping("add", evaluated),
            Callee::Builtin(Builtin::WrappingSub) => self.wrapping("sub", evaluated),
            Callee::Builtin(Builtin::WrappingMul) => self.wrapping("mul", evaluated),
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
            // `int_of(b: byte) -> int` widens, always defined: every
            // `byte` is 0..255, so zero-extension is exact -- the direct
            // counterpart of `lex-sys-codegen`'s `uextend`.
            Callee::Builtin(Builtin::IntOf) => {
                let byte = evaluated
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or_else(|| "`int_of` needs a byte argument".to_owned())?;
                let widened = self.fresh();
                self.out.push_str(&format!("  {widened} = zext i8 {} to i64\n", operand(&byte)));
                Ok(vec![LValue::Reg(widened)])
            }
            // `byte_of(n: int) -> byte`: narrow or trap (§7.5, `docs/
            // strings.md` §2) -- truncation is the silently wrong answer
            // `docs/defined-behaviour.md` §2.1 already refuses. One
            // unsigned comparison covers both ends, the same trick
            // `element_address`'s own bounds check already uses: a
            // negative `int` read as unsigned is far past 255.
            Callee::Builtin(Builtin::ByteOf) => {
                let n = evaluated
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or_else(|| "`byte_of` needs an int argument".to_owned())?;
                let out_of_range = self.fresh();
                self.out
                    .push_str(&format!("  {out_of_range} = icmp ugt i64 {}, 255\n", operand(&n)));
                self.trap_if(&out_of_range)?;
                let narrowed = self.fresh();
                self.out.push_str(&format!("  {narrowed} = trunc i64 {} to i8\n", operand(&n)));
                Ok(vec![LValue::Reg(narrowed)])
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
                match ret_kinds.as_slice() {
                    [] => {
                        self.out.push_str(&format!(
                            "  call void @lexs_{}({})\n",
                            target.name,
                            printed.join(", ")
                        ));
                        Ok(Vec::new())
                    }
                    [kind] => {
                        let result = self.fresh();
                        self.out.push_str(&format!(
                            "  {result} = call {} @lexs_{}({})\n",
                            kind.llvm(),
                            target.name,
                            printed.join(", ")
                        ));
                        Ok(vec![LValue::Reg(result)])
                    }
                    // A multi-leaf return comes back as one aggregate
                    // (`emit`'s own `struct_ty`), unpacked here the same
                    // way `checked_arith` already reads `{i64, i1}` back
                    // out of LLVM's overflow intrinsics.
                    kinds => {
                        let ty = struct_ty(kinds);
                        let agg = self.fresh();
                        self.out.push_str(&format!(
                            "  {agg} = call {ty} @lexs_{}({})\n",
                            target.name,
                            printed.join(", ")
                        ));
                        let mut unpacked = Vec::with_capacity(kinds.len());
                        for i in 0..kinds.len() {
                            let reg = self.fresh();
                            self.out.push_str(&format!("  {reg} = extractvalue {ty} {agg}, {i}\n"));
                            unpacked.push(LValue::Reg(reg));
                        }
                        Ok(unpacked)
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
