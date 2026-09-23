//! Control flow: `borrow`, assignment, `if`, `while` and `match`.

use crate::*;

impl<'a, 'f> BodyEmitter<'a, 'f> {
    pub(crate) fn borrow_stmt(
        &mut self,
        referent: Slot,
        reference: Slot,
        unique: bool,
        body: &[Stmt],
    ) -> bool {
        let ty = self.func.slots[referent.0 as usize].clone();
        let buffer = self.return_buffer(&ty);
        let base = self.slot_base[referent.0 as usize];
        let count = leaf_count(&ty, self.program, self.pointer);
        let values: Vec<Value> = (0..count)
            .map(|offset| self.builder.use_var(Variable::from_u32(base + offset)))
            .collect();
        self.store_leaves(buffer, &values);
        self.builder.def_var(Variable::from_u32(self.slot_base[reference.0 as usize]), buffer);
        let returned = self.stmts(body);

        // A unique borrow may have written through the reference, and the
        // variables still hold what the buffer held on the way in. Reading
        // them back is sound because the checker *locked* the referent for
        // the whole block: nothing else could touch it, so the buffer is the
        // only version that moved.
        //
        // Skipped when the body returned, because nothing after this runs --
        // and Cranelift has no block to put the loads in.
        if unique && !returned {
            let kinds = leaves(&ty, self.program, self.pointer);
            let restored = self.load_leaves(buffer, &kinds);
            for (offset, value) in restored.into_iter().enumerate() {
                self.builder.def_var(Variable::from_u32(base + offset as u32), value);
            }
        }
        returned
    }

    /// Write leaf values into a place.
    ///
    /// A whole local is `def_var` per leaf, as it always was. A field through
    /// a reference is the same arithmetic as reading one, running the other
    /// way: find where the field starts among the referent's leaves, and
    /// store there.
    pub(crate) fn write(&mut self, place: &Place, values: Vec<Value>) {
        match place {
            Place::Slot(slot) => {
                let base = self.slot_base[slot.0 as usize];
                for (offset, value) in values.into_iter().enumerate() {
                    self.builder.def_var(Variable::from_u32(base + offset as u32), value);
                }
            }
            Place::Element { base, index, element } => {
                let address = self.element_address(base, index, element);
                self.store_leaves(address, &values);
            }
            // `*r = e` — the whole referent replaced, at the address the
            // reference holds (`docs/reading-references.md` §3).
            Place::Deref { base, .. } => {
                let address = self.scalar(base);
                self.store_leaves(address, &values);
            }
            Place::Field { base, def, args, index } => {
                let address = self.scalar(base);
                let TypeInfo::Struct { fields, .. } = self.program.type_info(*def) else {
                    unreachable!("a field write to an enum should have been refused");
                };
                let start: u32 = fields[..*index as usize]
                    .iter()
                    .map(|(_, ty)| {
                        leaf_count(&ty.substitute(args, &[]), self.program, self.pointer)
                    })
                    .sum();
                let offset = start as i32 * RETURN_SLOT_STRIDE;
                for (i, value) in values.into_iter().enumerate() {
                    let at = offset + i as i32 * RETURN_SLOT_STRIDE;
                    self.builder.ins().store(MemFlags::trusted(), value, address, at);
                }
            }
        }
    }

    pub(crate) fn if_stmt(&mut self, cond: &Expr, then_body: &[Stmt], else_body: &[Stmt]) -> bool {
        let cond = self.scalar(cond);
        let then_block = self.builder.create_block();
        let else_block = self.builder.create_block();
        let merge = self.builder.create_block();

        // The condition is an `i8` holding 0 or 1, which is what `brif` tests.
        // M0 tested any integer for non-zero; the checker now guarantees a
        // `bool` got here.
        self.builder.ins().brif(cond, then_block, &[], else_block, &[]);

        self.builder.switch_to_block(then_block);
        self.builder.seal_block(then_block);
        if !self.stmts(then_body) {
            self.builder.ins().jump(merge, &[]);
        }

        self.builder.switch_to_block(else_block);
        self.builder.seal_block(else_block);
        if !self.stmts(else_body) {
            self.builder.ins().jump(merge, &[]);
        }

        let both_returned = terminates(then_body) && !else_body.is_empty() && terminates(else_body);
        self.builder.switch_to_block(merge);
        self.builder.seal_block(merge);
        if both_returned {
            // Nothing branches here. Fill it anyway: an empty block is not a
            // legal function, and this costs instructions the linker drops.
            self.return_zero();
        }
        both_returned
    }

    pub(crate) fn while_stmt(&mut self, cond: &Expr, body: &[Stmt]) {
        let header = self.builder.create_block();
        let body_block = self.builder.create_block();
        let exit = self.builder.create_block();

        self.builder.ins().jump(header, &[]);
        // The header stays unsealed until the back edge is emitted.
        self.builder.switch_to_block(header);
        let cond = self.scalar(cond);
        self.builder.ins().brif(cond, body_block, &[], exit, &[]);

        self.builder.switch_to_block(body_block);
        self.builder.seal_block(body_block);
        if !self.stmts(body) {
            self.builder.ins().jump(header, &[]);
        }
        self.builder.seal_block(header);

        self.builder.switch_to_block(exit);
        self.builder.seal_block(exit);
    }

    /// Where a variant's payload starts among an enum's leaves, and how many
    /// leaves each payload position occupies.
    /// Where component `index` of a tuple sits, in leaves: its start and
    /// its width (`docs/tuples.md` §5).
    ///
    /// The same sum a struct field needs, without a substitution step: a
    /// tuple's components are types already, not members written in terms
    /// of parameters that have to be filled in first.
    pub(crate) fn tuple_slice(&self, components: &[Type], index: u32) -> (u32, u32) {
        let start: u32 = components[..index as usize]
            .iter()
            .map(|ty| leaf_count(ty, self.program, self.pointer))
            .sum();
        (start, leaf_count(&components[index as usize], self.program, self.pointer))
    }

    pub(crate) fn variant_layout(
        &self,
        def: DefId,
        args: &[Type],
        variant: u32,
    ) -> (u32, Vec<u32>) {
        let TypeInfo::Enum { variants, .. } = self.program.type_info(def) else {
            unreachable!("a variant of a struct should have been refused");
        };
        let width = |ty: &Type| leaf_count(&ty.substitute(args, &[]), self.program, self.pointer);
        // One for the tag, then every earlier variant's payload.
        let mut offset = 1;
        for (_, payload) in &variants[..variant as usize] {
            offset += payload.iter().map(&width).sum::<u32>();
        }
        let widths = variants[variant as usize].1.iter().map(&width).collect();
        (offset, widths)
    }

    /// Lower a `match` to a chain of tag tests.
    ///
    /// A jump table would be faster and is the obvious later move; a chain is
    /// what M1 needs and is easier to be sure of. Returns whether every arm
    /// returned, which makes the whole `match` a terminator.
    pub(crate) fn match_stmt(
        &mut self,
        scrutinee: &Expr,
        def: DefId,
        args: &[Type],
        arms: &[Arm],
        by_reference: bool,
    ) -> bool {
        let values = self.expr(scrutinee);
        // Through a reference the scrutinee is one pointer, so the tag is a
        // load rather than a leaf already in hand
        // (`docs/reading-references.md` §2).
        let tag = if by_reference {
            self.builder.ins().load(types::I64, MemFlags::trusted(), values[0], 0)
        } else {
            values[0]
        };
        let merge = self.builder.create_block();

        let mut all_returned = true;
        // Whether the fall-through chain still has an open block. A wildcard
        // arm closes it, because nothing can follow one.
        let mut open = true;

        for arm in arms {
            if !open {
                break;
            }
            match arm.variant {
                Some(variant) => {
                    let body_block = self.builder.create_block();
                    let next = self.builder.create_block();
                    let matched =
                        self.builder.ins().icmp_imm(IntCC::Equal, tag, i64::from(variant));
                    self.builder.ins().brif(matched, body_block, &[], next, &[]);

                    self.builder.switch_to_block(body_block);
                    self.builder.seal_block(body_block);
                    self.bind_payload(def, args, variant, arm, &values, by_reference);
                    let returned = self.stmts(&arm.body);
                    if !returned {
                        self.builder.ins().jump(merge, &[]);
                    }
                    all_returned &= returned;

                    self.builder.switch_to_block(next);
                    self.builder.seal_block(next);
                }
                None => {
                    // The wildcard binds nothing and needs no test: it runs
                    // right here, in the block the chain fell through to.
                    let returned = self.stmts(&arm.body);
                    if !returned {
                        self.builder.ins().jump(merge, &[]);
                    }
                    all_returned &= returned;
                    open = false;
                }
            }
        }

        if open {
            // The checker proved the arms exhaustive, so this is unreachable.
            // It still needs filling: an unterminated block is not a legal
            // function.
            self.builder.ins().jump(merge, &[]);
        }

        self.builder.switch_to_block(merge);
        self.builder.seal_block(merge);
        if all_returned {
            self.return_zero();
        }
        all_returned
    }

    /// Copy a matched variant's payload into the slots its pattern bound.
    pub(crate) fn bind_payload(
        &mut self,
        def: DefId,
        args: &[Type],
        variant: u32,
        arm: &Arm,
        values: &[Value],
        by_reference: bool,
    ) {
        let (offset, widths) = self.variant_layout(def, args, variant);
        let mut at = offset as usize;
        for (binding, width) in arm.bindings.iter().zip(widths) {
            if let Some(slot) = binding {
                let base = self.slot_base[slot.0 as usize];
                if by_reference {
                    // A reference is one leaf -- except to a slice, and an
                    // unsized payload is refused long before here. Asserted
                    // rather than assumed, because the write below fills
                    // exactly one variable.
                    debug_assert_eq!(
                        leaf_count(&self.func.slots[slot.0 as usize], self.program, self.pointer),
                        1,
                        "a binding through a matched reference is one pointer"
                    );
                    // Each binding is an *address* inside the referent, not
                    // a copy of what is there: `values[0]` is where the enum
                    // lives and this payload starts `at` leaves into it. No
                    // load at all, which is why several unique references
                    // out of one match cost nothing (§2.2).
                    let address = self
                        .builder
                        .ins()
                        .iadd_imm(values[0], at as i64 * i64::from(RETURN_SLOT_STRIDE));
                    self.builder.def_var(Variable::from_u32(base), address);
                } else {
                    for index in 0..width {
                        let value = values[at + index as usize];
                        self.builder.def_var(Variable::from_u32(base + index), value);
                    }
                }
            }
            // A `_` binding still occupies its payload position; there is
            // simply nowhere to put the value.
            at += width as usize;
        }
    }
}
