//! Control flow: `borrow`, `if`, `while` and `match` (plus the
//! struct/enum layout `match`/`Expr::Enum` share).

use crate::*;

impl<'a> FuncEmitter<'a> {
    pub(crate) fn borrow_stmt(
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
    pub(crate) fn if_stmt(
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
    pub(crate) fn while_stmt(&mut self, cond: &Expr, body: &[Stmt]) -> Result<(), String> {
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
    pub(crate) fn variant_layout(
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
    pub(crate) fn enum_lit(
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
    /// -- `lex-sys-codegen`'s own `bind_payload`, both halves. By value,
    /// each bound leaf is copied out of `values` (already loaded by the
    /// caller). By reference (`docs/reading-references.md` §2), nothing
    /// is loaded at all: `values[0]` is the scrutinee's own pointer, and
    /// a binding gets the *address* of its payload position --
    /// `getelementptr`, the same idiom `Expr::FieldAddr` already uses --
    /// stored into the one-leaf slot a reference always is. A `_`
    /// binding still occupies its payload position either way; there is
    /// simply nowhere to put the value (or the address).
    pub(crate) fn bind_payload(
        &mut self,
        def: DefId,
        args: &[Type],
        variant: u32,
        arm: &Arm,
        values: &[LValue],
        by_reference: bool,
    ) -> Result<(), String> {
        let (offset, widths) = self.variant_layout(def, args, variant)?;
        let mut at = offset;
        for (binding, width) in arm.bindings.iter().zip(widths) {
            if let Some(slot) = binding {
                if by_reference {
                    if width != 1 {
                        return Err(
                            "matching through a reference cannot bind a payload wider than \
                             one leaf"
                                .to_owned(),
                        );
                    }
                    let addr = self.fresh();
                    self.out.push_str(&format!(
                        "  {addr} = getelementptr i8, ptr {}, i64 {}\n",
                        operand(&values[0]),
                        at as i64 * 8
                    ));
                    self.out.push_str(&format!(
                        "  store ptr {addr}, ptr {}\n",
                        Self::slot_reg(slot.0, 0)
                    ));
                } else {
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
            }
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
    pub(crate) fn match_stmt(
        &mut self,
        scrutinee: &Expr,
        def: DefId,
        args: &[Type],
        arms: &[Arm],
        by_reference: bool,
    ) -> Result<bool, String> {
        // A reference is always one pointer leaf (`Type::Ref`'s own rule
        // in `leaves_into`), whatever it refers to -- so `scrutinee`
        // evaluates the same way in both modes, and only reading the tag
        // out of it differs: by value it is already the tag, loaded;
        // by reference it is the scrutinee's own address, and the tag is
        // one more load away (`docs/reading-references.md` §2).
        let values = self.expr(scrutinee)?;
        let Some(first) = values.first().cloned() else {
            return Err("a match scrutinee must be an enum (at least one leaf: the tag)".to_owned());
        };
        let tag = if by_reference {
            let loaded = self.fresh();
            self.out.push_str(&format!("  {loaded} = load i64, ptr {}\n", operand(&first)));
            LValue::Reg(loaded)
        } else {
            first
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
                    self.bind_payload(def, args, variant, arm, &values, by_reference)?;
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

    /// The same offset arithmetic as [`Self::field_offset`], for a
    /// tuple: `components` carries the element types directly, so there
    /// is no declaration to look up (`Expr::TupleField`'s own reasoning,
    /// applied here to `Expr::TupleFieldRef`/`Expr::TupleFieldAddr`).
    pub(crate) fn tuple_field_offset(
        &self,
        components: &[Type],
        index: u32,
    ) -> Result<(i64, Vec<LKind>), String> {
        let mut start = 0i64;
        for ty in &components[..index as usize] {
            start += leaves_of(ty, self.program)?.len() as i64;
        }
        let kinds = leaves_of(&components[index as usize], self.program)?;
        Ok((start * 8, kinds))
    }
}
