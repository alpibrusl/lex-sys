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

use lex_sys_ir::{Builtin, Callee, Expr, Func, Place, Program, Slot, Stmt};
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
    text.push_str("declare i32 @putchar(i32)\n\n");

    for (index, func) in program.funcs.iter().enumerate() {
        let body = FuncEmitter::new(program, func)
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
    out: String,
    temp: u32,
}

impl<'a> FuncEmitter<'a> {
    fn new(program: &'a Program, func: &'a Func) -> Result<Self, String> {
        let slot_kinds =
            func.slots.iter().map(|ty| leaves_of(ty, program)).collect::<Result<Vec<_>, _>>()?;
        Ok(FuncEmitter { program, func, slot_kinds, out: String::new(), temp: 0 })
    }

    fn fresh(&mut self) -> String {
        let name = format!("%t{}", self.temp);
        self.temp += 1;
        name
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
                Stmt::If { .. } | Stmt::While { .. } | Stmt::Match { .. } | Stmt::Region { .. } => {
                    return Err("control flow is not part of the LLVM backend's first slice \
                         (docs/llvm-backend.md §5); `--backend cranelift` builds this program"
                        .to_owned());
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
