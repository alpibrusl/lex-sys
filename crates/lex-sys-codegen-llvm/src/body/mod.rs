//! One function's body. `FuncEmitter` is one type, and its `impl` is
//! split by concern across this module's children, which see its
//! private fields -- the same shape `lex-sys-codegen`'s own
//! `body/mod.rs` already splits `BodyEmitter` into, done here because
//! `docs/llvm-backend.md` §7.17's float slice pushed `emit.rs` past
//! `CONTRIBUTING.md`'s 2,000-line file budget.

use crate::*;

mod arith;
mod control;
mod expr;
mod memory;

pub(crate) struct FuncEmitter<'a> {
    pub(crate) program: &'a Program,
    pub(crate) func: &'a Func,
    /// Per slot, the LLVM type of each of its leaves.
    pub(crate) slot_kinds: Vec<Vec<LKind>>,
    pub(crate) triple: &'a Triple,
    /// Module-level constant declarations, shared across every function's
    /// `FuncEmitter` (§5's fourth slice): a string literal's global lives
    /// here, not in `out`, because a global declaration is not valid
    /// inside a function body.
    pub(crate) globals: &'a mut String,
    /// Shared across every function, so two literals in two different
    /// functions still get two distinct symbol names.
    pub(crate) next_literal: &'a mut u32,
    pub(crate) out: String,
    pub(crate) temp: u32,
    /// Numbers each `trap`/`ok` block pair a checked operator opens
    /// (§5's second slice) -- distinct from `temp`, which numbers SSA
    /// registers, because a block label and a register share no namespace
    /// in LLVM IR but reusing one counter for both would still be correct;
    /// two counters just read clearer in the emitted text.
    pub(crate) blocks: u32,
    /// Per open arena, the two `ptr`-typed `alloca` cells holding its base
    /// and its bump pointer (§7.5) -- `lex-sys-codegen`'s own `arenas:
    /// Vec<Option<(Variable, Variable)>>` (`body/memory.rs`), the same
    /// shape read through this backend's own memory-not-`Variable` idiom.
    /// Indexed by arena number, which `lex-sys-ir` assigns positionally
    /// (`docs/llvm-backend.md` §7.5), so a sibling region after an earlier
    /// one closed may need the vector grown rather than pushed to.
    pub(crate) arenas: Vec<Option<(String, String)>>,
}

impl<'a> FuncEmitter<'a> {
    pub(crate) fn new(
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

    pub(crate) fn fresh(&mut self) -> String {
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
    pub(crate) fn trap_asm(&self) -> Result<&'static str, String> {
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
    pub(crate) fn trap_if(&mut self, condition: &str) -> Result<(), String> {
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

    pub(crate) fn scalar(&mut self, expr: &Expr) -> Result<LValue, String> {
        let mut values = self.expr(expr)?;
        if values.len() != 1 {
            return Err(format!(
                "expected a single-leaf `int`/`bool`/`float` value here, got {} leaves",
                values.len()
            ));
        }
        Ok(values.remove(0))
    }

    /// This backend's `bool` leaf is `i8` (0 or 1); every branch needs
    /// LLVM's own `i1`, which this is the one conversion for.
    pub(crate) fn truthy(&mut self, value: &LValue) -> String {
        let cond = self.fresh();
        self.out.push_str(&format!("  {cond} = icmp ne i8 {}, 0\n", operand(value)));
        cond
    }

    pub(crate) fn slot_reg(slot: u32, leaf: u32) -> String {
        format!("%s{slot}_{leaf}")
    }

    /// A zero of the function's own return type, for a block the checker
    /// proved unreachable but LLVM still requires well-formed -- the
    /// function's own fall-through at the end of `emit`, and `match_stmt`'s
    /// impossible tag-chain fall-through, need exactly the same thing.
    pub(crate) fn emit_default_return(&mut self) -> Result<(), String> {
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
    pub(crate) fn pack_struct(&mut self, kinds: &[LKind], values: &[LValue]) -> String {
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

    pub(crate) fn emit(&mut self) -> Result<String, String> {
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

    pub(crate) fn stmts(&mut self, stmts: &[Stmt]) -> Result<bool, String> {
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
                // `*r = e` -- the whole referent replaced, at the address
                // the reference holds (`docs/reading-references.md` §3,
                // §7.11).
                Stmt::Store { place: Place::Deref { base, ty }, value } => {
                    let values = self.expr(value)?;
                    let address = self.scalar(base)?;
                    let kinds = leaves_of(ty, self.program)?;
                    self.store_leaves(&operand(&address), &kinds, &values);
                }
                // A field through a reference is the same arithmetic as
                // reading one, running the other way (`lex-sys-codegen`'s
                // own comment on `write`, `body/control.rs`, §7.11): find
                // where the field starts among the referent's leaves,
                // and store there.
                Stmt::Store { place: Place::Field { base, def, args, index }, value } => {
                    let values = self.expr(value)?;
                    let address = self.scalar(base)?;
                    let (offset, kinds) = self.field_offset(*def, args, *index)?;
                    let addr = self.fresh();
                    self.out.push_str(&format!(
                        "  {addr} = getelementptr i8, ptr {}, i64 {offset}\n",
                        operand(&address)
                    ));
                    self.store_leaves(&addr, &kinds, &values);
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
}
