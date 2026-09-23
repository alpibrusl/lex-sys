//! Expressions: the one large dispatch, and the operators.

use crate::*;

impl<'a, 'f> BodyEmitter<'a, 'f> {
    /// An expression whose type has exactly one leaf.
    pub(crate) fn scalar(&mut self, expr: &Expr) -> Value {
        let values = self.expr(expr);
        debug_assert_eq!(values.len(), 1, "expected a scalar, got {} leaves", values.len());
        values[0]
    }

    /// Emit an expression as its leaf values, in field order.
    ///
    /// A scalar yields one value and a struct yields one per leaf field, which
    /// is why this returns a vector rather than a `Value`: there is no single
    /// register a struct lives in, because it does not live in memory either.
    pub(crate) fn expr(&mut self, expr: &Expr) -> Vec<Value> {
        match expr {
            Expr::Int(v) => vec![self.builder.ins().iconst(types::I64, *v)],
            // From bits rather than from a decimal string, so the constant
            // in the object file is the one the parser read
            // (`docs/floating-point.md` §1).
            Expr::Float(bits) => {
                vec![self.builder.ins().f64const(f64::from_bits(*bits))]
            }
            Expr::Bool(v) => vec![self.builder.ins().iconst(types::I8, i64::from(*v))],
            Expr::Load(slot) => {
                let base = self.slot_base[slot.0 as usize];
                let count =
                    leaf_count(&self.func.slots[slot.0 as usize], self.program, self.pointer);
                (0..count)
                    .map(|offset| self.builder.use_var(Variable::from_u32(base + offset)))
                    .collect()
            }
            Expr::Struct { fields, .. } => {
                fields.iter().flat_map(|field| self.expr(field)).collect()
            }
            // Positional already, so unlike a struct there is nothing to
            // reorder: the components arrive in the order they were
            // written, which is the order they lie in.
            Expr::Tuple { parts } => parts.iter().flat_map(|part| self.expr(part)).collect(),
            Expr::TupleField { base, components, index } => {
                let values = self.expr(base);
                let (start, len) = self.tuple_slice(components, *index);
                values[start as usize..(start + len) as usize].to_vec()
            }
            Expr::Field { base, def, args, index } => {
                let values = self.expr(base);
                let TypeInfo::Struct { fields, .. } = self.program.type_info(*def) else {
                    unreachable!("a field access on an enum should have been refused");
                };
                let start: u32 = fields[..*index as usize]
                    .iter()
                    .map(|(_, ty)| {
                        leaf_count(&ty.substitute(args, &[]), self.program, self.pointer)
                    })
                    .sum();
                let len = leaf_count(
                    &fields[*index as usize].1.substitute(args, &[]),
                    self.program,
                    self.pointer,
                );
                values[start as usize..(start + len) as usize].to_vec()
            }
            // The same field arithmetic as `Expr::Field`, except the leaves
            // are loaded out of the buffer the reference points at rather
            // than picked out of leaves already in registers.
            Expr::Alloc { arena, ty, value } => vec![self.alloc(*arena, ty, value)],
            Expr::Boxed { ty, value } => vec![self.boxed(ty, value)],
            Expr::BoxedSlice { element, count, fill } => self.boxed_slice(element, count, fill),
            // One `free`, and the element count back. The pointer is the
            // first leaf and the length the second (§2).
            Expr::UnboxedSlice { value } => {
                let leaves = self.expr(value);
                self.free(leaves[0]);
                vec![leaves[1]]
            }
            Expr::Unboxed { ty, value } => self.unboxed(ty, value),
            // One load. A reference to a box points at where the box's own
            // pointer lives, so reading it *is* the reference to what the
            // box holds -- same region, same mode, nothing to check.
            // `*r` — the leaves at the address, loaded
            // (`docs/reading-references.md` §3). The same arithmetic a
            // field access does, over the whole referent rather than one
            // member of it.
            Expr::Deref { ty, value } => {
                let address = self.scalar(value);
                let kinds = leaves(ty, self.program, self.pointer);
                self.load_leaves(address, &kinds)
            }
            Expr::Contents { ty, value } => {
                let reference = self.scalar(value);
                let pointer = self.pointer;
                let held = self.builder.ins().load(pointer, MemFlags::trusted(), reference, 0);
                // A boxed slice's second leaf is its length, and `&r [T]`
                // is that same pair -- which is why one dereference serves
                // both shapes (`docs/boxed-slices.md` §3).
                if matches!(ty, Type::Slice(_)) {
                    let length = self.builder.ins().load(
                        types::I64,
                        MemFlags::trusted(),
                        reference,
                        RETURN_SLOT_STRIDE,
                    );
                    vec![held, length]
                } else {
                    vec![held]
                }
            }
            Expr::AllocSlice { arena, element, count, fill } => {
                self.alloc_slice(*arena, element, count, fill)
            }
            Expr::Index { base, index, element } => {
                let address = self.element_address(base, index, element);
                let kinds = leaves(element, self.program, self.pointer);
                self.load_leaves(address, &kinds)
            }
            Expr::Subslice { base, start, end, element } => {
                let (base, start, end, element) =
                    (base.clone(), start.clone(), end.clone(), element.clone());
                self.subslice(&base, &start, &end, &element)
            }
            // The length is the slice's second leaf: already there, never
            // computed.
            Expr::Len(slice) => vec![self.expr(slice)[1]],
            Expr::Bytes(text) => {
                let text = text.clone();
                self.bytes(&text)
            }
            // `docs/compile-time-data.md` §2: the same two leaves a
            // literal is, because that is what it became. The bytes were
            // computed rather than written, and by here nothing can tell.
            Expr::Static(index) => self.static_data(*index),
            Expr::FileOp { write, prefix, args } => {
                let (write, prefix, args) = (*write, prefix.clone(), args.clone());
                self.file_op(write, &prefix, &args)
            }
            Expr::OpenFile { prefix, args } => {
                let (prefix, args) = (prefix.clone(), args.clone());
                self.open_file(&prefix, &args)
            }
            Expr::Connect { bound, args } => {
                let (bound, args) = (bound.clone(), args.clone());
                self.connect(&bound, &args)
            }
            Expr::Bind { bound, args } => {
                let (bound, args) = (bound.clone(), args.clone());
                self.bind(&bound, &args)
            }
            Expr::FieldRef { base, def, args, index } => {
                let address = self.scalar(base);
                let TypeInfo::Struct { fields, .. } = self.program.type_info(*def) else {
                    unreachable!("a field access on an enum should have been refused");
                };
                let start: u32 = fields[..*index as usize]
                    .iter()
                    .map(|(_, ty)| {
                        leaf_count(&ty.substitute(args, &[]), self.program, self.pointer)
                    })
                    .sum();
                let kinds = leaves(
                    &fields[*index as usize].1.substitute(args, &[]),
                    self.program,
                    self.pointer,
                );
                let offset = start as i32 * RETURN_SLOT_STRIDE;
                kinds
                    .iter()
                    .enumerate()
                    .map(|(i, kind)| {
                        let at = offset + i as i32 * RETURN_SLOT_STRIDE;
                        self.builder.ins().load(*kind, MemFlags::trusted(), address, at)
                    })
                    .collect()
            }
            // A reference *to* the field: the same arithmetic as
            // `Expr::FieldRef`, stopping one step earlier
            // (`docs/reading-references.md` §2.0). That node loads the
            // field's leaves from `address + offset`; this one is the
            // address itself, so one `iadd_imm` replaces the loads.
            Expr::FieldAddr { base, def, args, index } => {
                let address = self.scalar(base);
                let TypeInfo::Struct { fields, .. } = self.program.type_info(*def) else {
                    unreachable!("a field access on an enum should have been refused");
                };
                let start: u32 = fields[..*index as usize]
                    .iter()
                    .map(|(_, ty)| {
                        leaf_count(&ty.substitute(args, &[]), self.program, self.pointer)
                    })
                    .sum();
                let offset = start as i64 * RETURN_SLOT_STRIDE as i64;
                vec![self.builder.ins().iadd_imm(address, offset)]
            }
            // The same, for a type with no declaration to consult.
            Expr::TupleFieldAddr { base, components, index } => {
                let address = self.scalar(base);
                let (start, _) = self.tuple_slice(components, *index);
                let offset = start as i64 * RETURN_SLOT_STRIDE as i64;
                vec![self.builder.ins().iadd_imm(address, offset)]
            }
            // The same arithmetic as `Expr::FieldRef`, over a type with no
            // declaration to consult: the component types travel with the
            // node instead.
            Expr::TupleFieldRef { base, components, index } => {
                let address = self.scalar(base);
                let (start, _) = self.tuple_slice(components, *index);
                let kinds = leaves(&components[*index as usize], self.program, self.pointer);
                let offset = start as i32 * RETURN_SLOT_STRIDE;
                kinds
                    .iter()
                    .enumerate()
                    .map(|(i, kind)| {
                        let at = offset + i as i32 * RETURN_SLOT_STRIDE;
                        self.builder.ins().load(*kind, MemFlags::trusted(), address, at)
                    })
                    .collect()
            }
            Expr::Enum { def, args, variant, payload } => {
                let whole = Type::Named(*def, args.clone());
                let (offset, widths) = self.variant_layout(*def, args, *variant);
                let total = leaf_count(&whole, self.program, self.pointer);
                let payload: Vec<Vec<Value>> = payload.iter().map(|e| self.expr(e)).collect();
                let all = leaves(&whole, self.program, self.pointer);

                // The tag, then every variant's leaves. This variant's are the
                // values just computed; the rest are zeroed, because a value
                // that is not this variant is not readable without matching on
                // the tag first.
                let mut out = Vec::with_capacity(total as usize);
                out.push(self.builder.ins().iconst(types::I64, i64::from(*variant)));
                for index in 1..total {
                    out.push(self.builder.ins().iconst(all[index as usize], 0));
                }
                let mut at = offset as usize;
                for (values, width) in payload.into_iter().zip(widths) {
                    debug_assert_eq!(values.len(), width as usize);
                    for value in values {
                        out[at] = value;
                        at += 1;
                    }
                }
                out
            }
            Expr::Neg(inner) => {
                // Negation overflows in exactly one place -- `-int::MIN` has
                // no positive counterpart -- so it is a checked subtraction
                // from zero rather than an `ineg` that would quietly hand
                // back `int::MIN` again.
                let v = self.scalar(inner);
                // A `float` has no such place: IEEE negation flips the
                // sign bit and is total, including on NaN and on zero,
                // where it is what produces `-0.0`
                // (`docs/floating-point.md` §2).
                if self.builder.func.dfg.value_type(v) == types::F64 {
                    return vec![self.builder.ins().fneg(v)];
                }
                let zero = self.builder.ins().iconst(types::I64, 0);
                let (value, overflowed) = self.builder.ins().ssub_overflow(zero, v);
                self.builder.ins().trapnz(overflowed, TrapCode::INTEGER_OVERFLOW);
                vec![value]
            }
            Expr::Not(inner) => {
                // A `bool` is 0 or 1, so flipping the low bit is the negation.
                let v = self.scalar(inner);
                vec![self.builder.ins().bxor_imm(v, 1)]
            }
            Expr::BitNot(inner) => {
                // Every bit, which is the whole difference from `Not` above:
                // that one knows its operand is 0 or 1 and this one does not
                // (`docs/bitwise.md` §1).
                let v = self.scalar(inner);
                vec![self.builder.ins().bnot(v)]
            }
            Expr::Bin { op, lhs, rhs } if op.is_short_circuit() => {
                vec![self.short_circuit(*op, lhs, rhs)]
            }
            Expr::Bin { op, lhs, rhs } => {
                let a = self.scalar(lhs);
                let b = self.scalar(rhs);
                vec![self.binary(*op, a, b)]
            }
            Expr::Call { callee, args } => {
                // Evaluated per argument rather than all at once, because a
                // builtin may take an argument it does not pass on: every
                // argument still runs, and only the values travel.
                let evaluated: Vec<Vec<Value>> = args.iter().map(|a| self.expr(a)).collect();
                let args: Vec<Value> = match callee {
                    Callee::Builtin(b) => {
                        evaluated.into_iter().skip(b.erased_args()).flatten().collect()
                    }
                    // A foreign function's capability parameters are the
                    // checker's business, not C's: they carry no data, so
                    // they stop here (§8.1). Every reference an `extern`
                    // declaration takes is a borrowed capability — the
                    // collector refuses any other — so dropping the
                    // references is exact whatever order they were written in.
                    Callee::Extern(index) => evaluated
                        .into_iter()
                        .zip(&self.program.externs[*index as usize].params)
                        .filter(|(_, param)| crosses_to_c(param))
                        .flat_map(|(values, _)| values)
                        .collect(),
                    Callee::Fn(_) => evaluated.into_iter().flatten().collect(),
                };
                match callee {
                    // §8.1: "capabilities erase at compile time except where
                    // they carry data". These two carry none, so there is
                    // nothing to emit — `split` hands back a value with no
                    // leaves and `release` ends one that was never there.
                    //
                    // The arguments are still evaluated above, because a
                    // capability's *journey* is what the checker tracked and
                    // an argument may have side effects on the way in.
                    Callee::Extern(index) => {
                        let ext = &self.program.externs[*index as usize];
                        let f = self
                            .module
                            .declare_func_in_func(self.foreign[*index as usize], self.builder.func);
                        let call = self.builder.ins().call(f, &args);
                        let results = self.builder.inst_results(call).to_vec();
                        if matches!(ext.ret, Type::Unit) { Vec::new() } else { results }
                    }
                    // `narrow` is a compile-time fact: the capability it
                    // returns names a smaller library than the one it
                    // consumed, and neither carries a bit at runtime (§7.4).
                    Callee::Builtin(Builtin::Split | Builtin::Narrow) => Vec::new(),
                    Callee::Builtin(Builtin::Release) => {
                        vec![self.builder.ins().iconst(types::I64, 0)]
                    }
                    Callee::Fn(id) => {
                        let callee = &self.program.funcs[id.0 as usize];
                        let ret = callee.ret.clone();
                        let f = self
                            .module
                            .declare_func_in_func(self.declared[id.0 as usize], self.builder.func);

                        if !returns_indirectly(&ret, self.program, self.pointer) {
                            let call = self.builder.ins().call(f, &args);
                            return self.builder.inst_results(call).to_vec();
                        }

                        // Too wide for registers: hand the callee somewhere to
                        // put it, then read it back.
                        let buffer = self.return_buffer(&ret);
                        let mut with_buffer = Vec::with_capacity(args.len() + 1);
                        with_buffer.push(buffer);
                        with_buffer.extend(args);
                        self.builder.ins().call(f, &with_buffer);
                        let kinds = leaves(&ret, self.program, self.pointer);
                        self.load_leaves(buffer, &kinds)
                    }
                    // The escape from checked arithmetic. `iadd`/`isub`/
                    // `imul` are two's-complement wraparound, which is what
                    // was asked for here — the checked forms above are the
                    // ones that trap.
                    Callee::Builtin(Builtin::WrappingAdd) => {
                        vec![self.builder.ins().iadd(args[0], args[1])]
                    }
                    Callee::Builtin(Builtin::WrappingSub) => {
                        vec![self.builder.ins().isub(args[0], args[1])]
                    }
                    Callee::Builtin(Builtin::WrappingMul) => {
                        vec![self.builder.ins().imul(args[0], args[1])]
                    }
                    // `len` never reaches here: it is checked and lowered
                    // at the call site, like `release` and `narrow`, because
                    // its argument's element type is what decides it.
                    Callee::Builtin(Builtin::Len) => {
                        unreachable!("`len` is lowered as `Expr::Len`")
                    }
                    // `docs/floating-point.md` §4: round-to-nearest-even,
                    // which is what `fcvt_from_sint` does. No check,
                    // because every `int` has a nearest `float`.
                    Callee::Builtin(Builtin::FloatOf) => {
                        vec![self.builder.ins().fcvt_from_sint(types::F64, args[0])]
                    }
                    // Toward zero, trapping on NaN, ±infinity and any
                    // magnitude at or past `2^63` -- exactly the inputs C
                    // leaves undefined (§4).
                    //
                    // Cranelift has both forms: `fcvt_to_sint` traps on
                    // precisely those, and `fcvt_to_sint_sat` saturates.
                    // Saturating is the silently wrong answer here, so the
                    // trapping one is the one that belongs.
                    Callee::Builtin(Builtin::Truncate) => {
                        vec![self.builder.ins().fcvt_to_sint(types::I64, args[0])]
                    }
                    // A reinterpretation, so `bitcast` and no arithmetic
                    // (`docs/float-printing.md` §2). The bits are the
                    // same sixty-four; only the type changes -- except for
                    // NaN, which answers one pattern on every target.
                    //
                    // IEEE-754 leaves a generated NaN's sign and payload to
                    // the hardware, and the two targets disagree: `0.0 /
                    // 0.0` is `0xfff8...` on x86-64 and `0x7ff8...` on
                    // aarch64, so a bare `bitcast` made `bits_of` the one
                    // operation whose answer depended on where the program
                    // ran (`docs/differential.md` §4). Nothing a program
                    // can do reaches a NaN's payload -- there is no
                    // `float_of_bits` -- so the only thing canonicalising
                    // loses is the hardware's accident.
                    Callee::Builtin(Builtin::BitsOf) => {
                        let x = args[0];
                        let raw = self.builder.ins().bitcast(types::I64, MemFlags::new(), x);
                        let nan = self.builder.ins().fcmp(FloatCC::Unordered, x, x);
                        let canonical = self.builder.ins().iconst(types::I64, CANONICAL_NAN);
                        vec![self.builder.ins().select(nan, canonical, raw)]
                    }
                    // `x != x`, which is true for NaN and nothing else.
                    // A riddle as an expression (§5), which is why it has
                    // a name.
                    Callee::Builtin(Builtin::IsNan) => {
                        let x = args[0];
                        vec![self.builder.ins().fcmp(FloatCC::NotEqual, x, x)]
                    }
                    // One instruction -- `sqrtsd` on x86-64, `fsqrt` on
                    // aarch64 -- and IEEE-754 requires it to be correctly
                    // rounded, which is why `docs/float-math.md` §2 says
                    // this cannot be library code: the instruction is the
                    // only correct implementation there is.
                    Callee::Builtin(Builtin::Sqrt) => {
                        vec![self.builder.ins().sqrt(args[0])]
                    }
                    // §2: narrow or trap. Truncation is the silently wrong
                    // answer `defined-behaviour.md` §2.1 already refused.
                    Callee::Builtin(Builtin::ByteOf) => {
                        let n = args[0];
                        // One unsigned comparison covers both ends, exactly
                        // as the bounds check does: a negative integer read
                        // as unsigned is enormous, so `n > 255` catches it.
                        let out_of_range =
                            self.builder.ins().icmp_imm(IntCC::UnsignedGreaterThan, n, 255);
                        self.builder.ins().trapnz(out_of_range, TrapCode::INTEGER_OVERFLOW);
                        vec![self.builder.ins().ireduce(types::I8, n)]
                    }
                    // Always defined, and always lands in 0..255 -- which is
                    // why it widens *unsigned* rather than sign-extending.
                    Callee::Builtin(Builtin::IntOf) => {
                        vec![self.builder.ins().uextend(types::I64, args[0])]
                    }
                    // Both are lowered as `Expr::FileOp`, which carries the
                    // prefix the backend checks against.
                    Callee::Builtin(Builtin::FsRead | Builtin::FsWrite) => {
                        unreachable!("a file operation is lowered as `Expr::FileOp`")
                    }
                    // Like the two above: the prefix decides the path check,
                    // so it travels in its own node.
                    Callee::Builtin(Builtin::OpenRead) => {
                        unreachable!("`open_read` is lowered as `Expr::OpenFile`")
                    }
                    // Like the two above: the bound travels with its own
                    // node (`docs/net.md` §4.1).
                    Callee::Builtin(Builtin::Connect) => {
                        unreachable!("`connect` is lowered as `Expr::Connect`")
                    }
                    Callee::Builtin(Builtin::Bind) => {
                        unreachable!("`bind` is lowered as `Expr::Bind`")
                    }
                    // Neither takes a capability, so both are ordinary
                    // fixed-signature calls (`docs/listen.md` §6).
                    Callee::Builtin(Builtin::Listen) => {
                        let listen =
                            self.libc_fn("listen", &[types::I32, types::I32], &[types::I32]);
                        let listen = self.module.declare_func_in_func(listen, self.builder.func);
                        let fd = self.builder.ins().ireduce(types::I32, args[0]);
                        let backlog = self.builder.ins().ireduce(types::I32, args[1]);
                        let call = self.builder.ins().call(listen, &[fd, backlog]);
                        let answer = self.builder.inst_results(call)[0];
                        vec![self.builder.ins().sextend(types::I64, answer)]
                    }
                    // The peer address is ignored -- `NULL, NULL` -- the
                    // same as `examples/serve/`'s own hand-written call.
                    Callee::Builtin(Builtin::Accept) => {
                        let pointer = self.pointer;
                        let accept =
                            self.libc_fn("accept", &[types::I32, pointer, pointer], &[types::I32]);
                        let accept = self.module.declare_func_in_func(accept, self.builder.func);
                        let fd = self.builder.ins().ireduce(types::I32, args[0]);
                        let null = self.builder.ins().iconst(pointer, 0);
                        let call = self.builder.ins().call(accept, &[fd, null, null]);
                        let answer = self.builder.inst_results(call)[0];
                        vec![self.builder.ins().sextend(types::I64, answer)]
                    }
                    // `docs/file-handles.md` §3. `read(2)` answers a count,
                    // zero at the end, and `-1` with the reason in `errno` --
                    // which is exactly the three outcomes `Read` has
                    // constructors for, so this is the one place the
                    // sentinel is unpacked and the last.
                    Callee::Builtin(Builtin::ReadFile) => self.read_file(&args),
                    // `close(2)`. The handle is one leaf and it ends here.
                    Callee::Builtin(Builtin::Close) => {
                        let close = self.libc_fn("close", &[types::I32], &[types::I32]);
                        let close = self.module.declare_func_in_func(close, self.builder.func);
                        let fd = self.builder.ins().ireduce(types::I32, args[0]);
                        let call = self.builder.ins().call(close, &[fd]);
                        let answer = self.builder.inst_results(call)[0];
                        vec![self.builder.ins().sextend(types::I64, answer)]
                    }
                    // Like `len` and the file operations: checked and
                    // lowered at the call site, because the type being boxed
                    // is what decides every one of them.
                    Callee::Builtin(
                        Builtin::Box
                        | Builtin::Unbox
                        | Builtin::Contents
                        | Builtin::BoxSlice
                        | Builtin::UnboxSlice,
                    ) => {
                        unreachable!("a heap operation is lowered as its own node")
                    }
                    // `docs/arguments.md` §3: `argc`, exactly as the
                    // runtime gave it.
                    Callee::Builtin(Builtin::ArgCount) => vec![self.argc()],
                    // One argument, as a pointer and a length. C hands over
                    // a NUL-terminated string; the terminator is an
                    // artifact of that interface rather than part of the
                    // value, so the length is computed and the NUL is left
                    // behind (§3.2).
                    Callee::Builtin(Builtin::Arg) => {
                        let pointer = self.pointer;
                        // The capability carries no leaves, so the index is
                        // the only argument that arrived.
                        let index = args[0];

                        // Outside `0 .. argc` traps, like indexing past a
                        // slice: it is the same mistake and gets the same
                        // answer. One unsigned comparison covers both ends.
                        let count = self.argc();
                        let past = self.builder.ins().icmp(
                            IntCC::UnsignedGreaterThanOrEqual,
                            index,
                            count,
                        );
                        self.builder.ins().trapnz(past, TrapCode::HEAP_OUT_OF_BOUNDS);

                        let argv = self.global(ARGV_GLOBAL);
                        let argv = self.builder.ins().load(pointer, MemFlags::trusted(), argv, 0);
                        let offset =
                            self.builder.ins().imul_imm(index, i64::from(RETURN_SLOT_STRIDE));
                        let slot = self.builder.ins().iadd(argv, offset);
                        let text = self.builder.ins().load(pointer, MemFlags::trusted(), slot, 0);

                        let strlen = self.libc_fn("strlen", &[pointer], &[types::I64]);
                        let strlen = self.module.declare_func_in_func(strlen, self.builder.func);
                        let call = self.builder.ins().call(strlen, &[text]);
                        let length = self.builder.inst_results(call)[0];
                        vec![text, length]
                    }
                    Callee::Builtin(Builtin::PutChar) => {
                        let f = self
                            .module
                            .declare_func_in_func(self.console.putchar, self.builder.func);
                        let arg = self.builder.ins().ireduce(types::I32, args[0]);
                        let call = self.builder.ins().call(f, &[arg]);
                        let result = self.builder.inst_results(call)[0];
                        vec![self.builder.ins().sextend(types::I64, result)]
                    }
                    // `docs/bulk-io.md` §3: the whole slice in one call,
                    // through the same stdio stream `putchar` uses — and
                    // `docs/standard-error.md` §3.3 is the same call on
                    // the other stream, one label apart in the type
                    // system and one symbol apart here.
                    Callee::Builtin(Builtin::Write | Builtin::WriteErr) => {
                        let pointer = self.pointer;
                        let (start, len) = (args[0], args[1]);
                        let target = match callee {
                            Callee::Builtin(Builtin::WriteErr) => self.console.stderr,
                            _ => self.console.stdout,
                        };
                        let stream = self.module.declare_data_in_func(target, self.builder.func);
                        let stream = self.builder.ins().global_value(pointer, stream);
                        // `stdout` and `stderr` are `FILE *` *variables*,
                        // so the symbol is the address of the pointer and
                        // the stream is one load away.
                        let stream =
                            self.builder.ins().load(pointer, MemFlags::trusted(), stream, 0);
                        let one = self.builder.ins().iconst(pointer, 1);
                        let f = self
                            .module
                            .declare_func_in_func(self.console.fwrite, self.builder.func);
                        let call = self.builder.ins().call(f, &[start, one, len, stream]);
                        vec![self.builder.inst_results(call)[0]]
                    }
                    // Sign-extended, not zero-extended: `EOF` is `-1` and
                    // zero-extending would hand the program 4294967295,
                    // which is a byte-range check that silently never
                    // fires.
                    Callee::Builtin(Builtin::GetChar) => {
                        let f = self
                            .module
                            .declare_func_in_func(self.console.getchar, self.builder.func);
                        let call = self.builder.ins().call(f, &[]);
                        let result = self.builder.inst_results(call)[0];
                        vec![self.builder.ins().sextend(types::I64, result)]
                    }
                }
            }
        }
    }

    /// `&&` and `||`, which are control flow rather than instructions: the
    /// right operand must not be evaluated when the left already decides the
    /// answer.
    ///
    /// The result travels in a variable rather than a block parameter, so this
    /// reuses the same SSA construction the slots already use.
    pub(crate) fn short_circuit(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Value {
        let result = self.temporary(types::I8);

        let rhs_block = self.builder.create_block();
        let merge = self.builder.create_block();

        let a = self.scalar(lhs);
        // Short-circuiting means the answer is the left operand itself.
        self.builder.def_var(result, a);
        match op {
            BinOp::And => self.builder.ins().brif(a, rhs_block, &[], merge, &[]),
            BinOp::Or => self.builder.ins().brif(a, merge, &[], rhs_block, &[]),
            other => unreachable!("`{other:?}` does not short-circuit"),
        };

        self.builder.switch_to_block(rhs_block);
        self.builder.seal_block(rhs_block);
        let b = self.scalar(rhs);
        self.builder.def_var(result, b);
        self.builder.ins().jump(merge, &[]);

        self.builder.switch_to_block(merge);
        self.builder.seal_block(merge);
        self.builder.use_var(result)
    }

    /// The IEEE-754 half of [`Self::binary`].
    ///
    /// No checks anywhere, which is the whole of §2.1's argument in code:
    /// `1.0 / 0.0` is infinity and `0.0 / 0.0` is NaN, both of them
    /// defined answers rather than the silently wrong ones wrapping
    /// arithmetic would hand back. The comparisons are the *ordered*
    /// forms, so every one of them is false when either side is NaN,
    /// which is what makes trichotomy fail (§5).
    pub(crate) fn float_binary(&mut self, op: BinOp, a: Value, b: Value) -> Value {
        let cc = match op {
            BinOp::Add => return self.builder.ins().fadd(a, b),
            BinOp::Sub => return self.builder.ins().fsub(a, b),
            BinOp::Mul => return self.builder.ins().fmul(a, b),
            BinOp::Div => return self.builder.ins().fdiv(a, b),
            BinOp::Eq => FloatCC::Equal,
            BinOp::Ne => FloatCC::NotEqual,
            BinOp::Lt => FloatCC::LessThan,
            BinOp::Le => FloatCC::LessThanOrEqual,
            BinOp::Gt => FloatCC::GreaterThan,
            BinOp::Ge => FloatCC::GreaterThanOrEqual,
            other => unreachable!("the checker refuses `{other:?}` on `float`"),
        };
        // `fcmp` already yields the `i8` a `bool` is here, exactly as
        // `icmp` does at the end of `binary` -- no widening.
        self.builder.ins().fcmp(cc, a, b)
    }

    pub(crate) fn binary(&mut self, op: BinOp, a: Value, b: Value) -> Value {
        // `docs/floating-point.md` §2: IEEE-754 binary64, which is a
        // different instruction for every operator. The checker has
        // already agreed the two sides, so one of them decides.
        if self.builder.func.dfg.value_type(a) == types::F64 {
            return self.float_binary(op, a, b);
        }
        let cc = match op {
            // `int` is 64-bit two's complement and arithmetic on it is
            // *checked*: a result that does not fit traps rather than
            // wrapping (`docs/defined-behaviour.md`). Wrapping silently is
            // not undefined behaviour, but it is a silently wrong answer,
            // and the whole point of dividing by zero trapping is that this
            // language does not hand those back. `wrapping_add` and its two
            // siblings are there for when wraparound is the intent.
            BinOp::Add => {
                let (value, overflowed) = self.builder.ins().sadd_overflow(a, b);
                self.builder.ins().trapnz(overflowed, TrapCode::INTEGER_OVERFLOW);
                return value;
            }
            BinOp::Sub => {
                let (value, overflowed) = self.builder.ins().ssub_overflow(a, b);
                self.builder.ins().trapnz(overflowed, TrapCode::INTEGER_OVERFLOW);
                return value;
            }
            BinOp::Mul => {
                let (value, overflowed) = self.builder.ins().smul_overflow(a, b);
                self.builder.ins().trapnz(overflowed, TrapCode::INTEGER_OVERFLOW);
                return value;
            }
            // Cranelift's `sdiv`/`srem` trap on a zero divisor and on
            // `int::MIN / -1`. A trap is defined behaviour; C's answer here is
            // not, which is the difference the language exists to make (#1).
            BinOp::Div => return self.builder.ins().sdiv(a, b),
            BinOp::Rem => return self.builder.ins().srem(a, b),
            // `docs/bitwise.md` §4: these cannot overflow, so unlike `+`
            // above there is nothing to check.
            BinOp::BitAnd => return self.builder.ins().band(a, b),
            BinOp::BitOr => return self.builder.ins().bor(a, b),
            BinOp::BitXor => return self.builder.ins().bxor(a, b),
            // A shift amount outside `0..64` traps (§3). Cranelift's
            // `ishl`/`sshr` *mask* the amount, which is the silently wrong
            // answer §2.1 refuses -- so the check is explicit here, and it
            // is one unsigned comparison because a negative amount is a
            // huge unsigned one.
            BinOp::Shl | BinOp::Shr => {
                let out_of_range =
                    self.builder.ins().icmp_imm(IntCC::UnsignedGreaterThanOrEqual, b, 64);
                self.builder.ins().trapnz(out_of_range, TrapCode::INTEGER_OVERFLOW);
                return match op {
                    BinOp::Shl => self.builder.ins().ishl(a, b),
                    // Arithmetic, because `int` is signed (§2).
                    _ => self.builder.ins().sshr(a, b),
                };
            }
            BinOp::Eq => IntCC::Equal,
            BinOp::Ne => IntCC::NotEqual,
            BinOp::Lt => IntCC::SignedLessThan,
            BinOp::Le => IntCC::SignedLessThanOrEqual,
            BinOp::Gt => IntCC::SignedGreaterThan,
            BinOp::Ge => IntCC::SignedGreaterThanOrEqual,
            other => unreachable!("`{other:?}` is not an instruction"),
        };
        // `icmp` yields an `i8` holding 0 or 1, which is exactly a `bool`.
        // M0 widened this to `i64`; nothing needs widening now.
        self.builder.ins().icmp(cc, a, b)
    }
}
