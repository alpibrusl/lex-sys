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
            Expr::F32(bits) => {
                vec![self.builder.ins().f32const(f32::from_bits(*bits))]
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
            // `docs/threads.md` §2: blocks until the thread `spawn`
            // started returns, then reads `R`'s own leaves back out of
            // what `pthread_join` wrote -- zero, for `()`, or one,
            // since `lower/conc.rs::spawn` already refused anything
            // wider.
            Expr::Joined { handle, ret } => {
                let pointer = self.pointer;
                let handle_value = self.scalar(handle);
                let join = self.libc_fn("pthread_join", &[pointer, pointer], &[types::I32]);
                let join_ref = self.module.declare_func_in_func(join, self.builder.func);
                let result_slot = self.return_buffer(&Type::Int);
                let call = self.builder.ins().call(join_ref, &[handle_value, result_slot]);
                let status = self.builder.inst_results(call)[0];
                let failed = self.builder.ins().icmp_imm(IntCC::NotEqual, status, 0);
                self.builder.ins().trapnz(failed, TrapCode::HEAP_OUT_OF_BOUNDS);
                // The thread has ended: `signals_watch` may be granted again
                // once the last one has (`docs/signals.md` section 3).
                self.count_thread(-1);
                match leaves(ret, self.program, pointer).as_slice() {
                    [] => Vec::new(),
                    [kind] => {
                        vec![self.builder.ins().load(*kind, MemFlags::trusted(), result_slot, 0)]
                    }
                    _ => unreachable!(
                        "`docs/threads.md` §1 restricts a thread's return to at most one leaf"
                    ),
                }
            }
            Expr::Static(index) => self.static_data(*index),
            Expr::FileOp { write, prefix, args } => {
                let (write, prefix, args) = (*write, prefix.clone(), args.clone());
                self.file_op(write, &prefix, &args)
            }
            Expr::OpenFile { prefix, mode, args } => {
                let (prefix, mode, args) = (prefix.clone(), *mode, args.clone());
                self.open_file(&prefix, mode, &args)
            }
            Expr::TtyOpen { prefix, args } => {
                let (prefix, args) = (prefix.clone(), args.clone());
                self.tty_open(&prefix, &args)
            }
            Expr::PathOp { op, prefix, args } => {
                let (op, prefix, args) = (*op, prefix.clone(), args.clone());
                self.path_op(op, &prefix, &args)
            }
            Expr::ExecSpawn { prefix, in_dir, args } => {
                let (prefix, in_dir, args) = (prefix.clone(), *in_dir, args.clone());
                self.exec_spawn(&prefix, in_dir, &args)
            }
            Expr::Connect { bound, args } => {
                let (bound, args) = (bound.clone(), args.clone());
                self.connect(&bound, &args)
            }
            Expr::Bind { bound, args } => {
                let (bound, args) = (bound.clone(), args.clone());
                self.bind(&bound, &args)
            }
            // `docs/native-sockets.md` §3: `bind`'s node with a handle for
            // an answer.
            Expr::TcpListen { bound, args, datagram } => {
                let (bound, args, datagram) = (bound.clone(), args.clone(), *datagram);
                self.tcp_listen(&bound, &args, datagram)
            }
            Expr::TcpConnect { bound, args, start, datagram } => {
                let (bound, args, start, datagram) =
                    (bound.clone(), args.clone(), *start, *datagram);
                self.tcp_connect(&bound, &args, start, datagram)
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
                if matches!(self.builder.func.dfg.value_type(v), types::F64 | types::F32) {
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
                        if matches!(ext.ret, Type::Unit) {
                            Vec::new()
                        } else if ext.narrow_return && matches!(ext.ret, Type::Int) {
                            // The call above declared a 32-bit return
                            // (`emit.rs`): sign-extend it back to this
                            // backend's own 64-bit `int` here, at the one
                            // place that read the real ABI width, rather
                            // than trust whatever the upper 32 bits of
                            // the return register happen to hold.
                            vec![self.builder.ins().sextend(types::I64, results[0])]
                        } else {
                            results
                        }
                    }
                    // `narrow` is a compile-time fact: the capability it
                    // returns names a smaller library than the one it
                    // consumed, and neither carries a bit at runtime (§7.4).
                    Callee::Builtin(
                        Builtin::Split | Builtin::Narrow | Builtin::ForkHeap | Builtin::ForkClock,
                    ) => Vec::new(),
                    Callee::Builtin(Builtin::Release) => {
                        vec![self.builder.ins().iconst(types::I64, 0)]
                    }
                    // `docs/opaque-pointers.md` §3: the null handle,
                    // pointer-width and zero like every other null
                    // pointer this backend already emits (`abi::leaves_
                    // into`'s own `Type::Ref` arm).
                    Callee::Builtin(Builtin::NullPtr) => {
                        vec![self.builder.ins().iconst(self.pointer, 0)]
                    }
                    // An explicit, deliberate trap (`docs/testing.md` §2)
                    // rather than a check the compiler inserted on its
                    // own -- a distinct user trap code, not one of the
                    // four reserved ones above, says so in a debugger.
                    // `trap` is a terminator, so the block it ends needs
                    // a fresh one after it; nothing reaches this block,
                    // but the call still has to answer with a value of
                    // the right shape.
                    Callee::Builtin(Builtin::Trap) => {
                        self.builder.ins().trap(TrapCode::unwrap_user(1));
                        let dead = self.builder.create_block();
                        self.builder.switch_to_block(dead);
                        self.builder.seal_block(dead);
                        vec![self.builder.ins().iconst(types::I64, 0)]
                    }
                    // `docs/threads.md` §2: `body`'s own compiled entry
                    // point (`args[1]`, `Expr::FnValue`'s address)
                    // becomes `pthread_create`'s start routine directly
                    // -- checked at the call site
                    // (`lower/conc.rs::spawn`) to be safe as one, so no
                    // trampoline is built here. `Thread[T, R]`'s own
                    // single leaf is the raw `pthread_t` `pthread_create`
                    // wrote into a stack slot this call owns.
                    Callee::Builtin(Builtin::Spawn) => {
                        let pointer = self.pointer;
                        // A `()` payload contributes zero leaves, so
                        // `args` (already skip-flattened above) holds
                        // only `body`'s own address; anything else
                        // contributes exactly one.
                        let (payload, start_routine) = if args.len() == 1 {
                            (self.builder.ins().iconst(pointer, 0), args[0])
                        } else {
                            let raw = args[0];
                            let widened = if self.builder.func.dfg.value_type(raw) == pointer {
                                raw
                            } else {
                                // A `byte`/`bool` payload is Cranelift
                                // `I8`; `pthread_create`'s signature
                                // declares every parameter
                                // `pointer`-width, and `call` requires
                                // an exact type match.
                                self.builder.ins().uextend(pointer, raw)
                            };
                            (widened, args[1])
                        };
                        let create = self.libc_fn(
                            "pthread_create",
                            &[pointer, pointer, pointer, pointer],
                            &[types::I32],
                        );
                        let create_ref =
                            self.module.declare_func_in_func(create, self.builder.func);
                        let thread_slot = self.return_buffer(&Type::Int);
                        let attr = self.builder.ins().iconst(pointer, 0);
                        let call = self
                            .builder
                            .ins()
                            .call(create_ref, &[thread_slot, attr, start_routine, payload]);
                        let status = self.builder.inst_results(call)[0];
                        // `body/memory.rs`'s own `malloc` check: a
                        // resource failure is a trap, not a value this
                        // program pretends is a real thread.
                        let failed = self.builder.ins().icmp_imm(IntCC::NotEqual, status, 0);
                        self.builder.ins().trapnz(failed, TrapCode::HEAP_OUT_OF_BOUNDS);
                        // A running thread forbids a new signal claim: it
                        // would not have the signals blocked
                        // (`docs/signals.md` section 3).
                        self.count_thread(1);
                        vec![self.builder.ins().load(pointer, MemFlags::trusted(), thread_slot, 0)]
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
                    //
                    // But `fcvt_to_sint` accepts exactly `-2^63`, which is a
                    // representable `i64`, and §4 says "any magnitude at or
                    // beyond `2^63`": found by `int_of_f32`'s test
                    // (`docs/f32.md` §5.2), the LLVM backend already
                    // refused it, so the lower bound is checked here too.
                    Callee::Builtin(Builtin::Truncate) => {
                        let x = args[0];
                        let low = self.builder.ins().f64const(-9_223_372_036_854_775_808.0);
                        let too_low = self.builder.ins().fcmp(FloatCC::LessThanOrEqual, x, low);
                        self.builder.ins().trapnz(too_low, TrapCode::INTEGER_OVERFLOW);
                        vec![self.builder.ins().fcvt_to_sint(types::I64, x)]
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
                    // ran (`docs/differential.md` §4). Arithmetic never
                    // reaches a NaN's payload, so what canonicalising loses
                    // is the hardware's accident -- and the payload a
                    // program built with `float_of_bits` (edition 7), which
                    // it can never read back: one pattern for every NaN.
                    // `bits_of`'s inverse: the same sixty-four bits, read as
                    // a float. No NaN is rewritten on the way in, so a
                    // program can build one with a payload; `bits_of` reads
                    // it back as the canonical pattern.
                    Callee::Builtin(Builtin::FloatOfBits) => {
                        vec![self.builder.ins().bitcast(types::F64, MemFlags::new(), args[0])]
                    }
                    Callee::Builtin(Builtin::BitsOf) => {
                        let x = args[0];
                        let raw = self.builder.ins().bitcast(types::I64, MemFlags::new(), x);
                        let nan = self.builder.ins().fcmp(FloatCC::Unordered, x, x);
                        let canonical = self.builder.ins().iconst(types::I64, CANONICAL_NAN);
                        vec![self.builder.ins().select(nan, canonical, raw)]
                    }
                    // `docs/f32.md` §2: `fdemote` is IEEE conversion to the
                    // narrower format under round-to-nearest-even, and an
                    // overflow is infinity, never a trap. `fpromote` is
                    // exact.
                    Callee::Builtin(Builtin::F32Of) => {
                        vec![self.builder.ins().fdemote(types::F32, args[0])]
                    }
                    Callee::Builtin(Builtin::FloatOf32) => {
                        vec![self.builder.ins().fpromote(types::F64, args[0])]
                    }
                    // As `BitsOf`, at 32 bits: a `bitcast`, with every NaN
                    // answering one pattern, then zero-extended so the
                    // answer is the unsigned 32-bit value.
                    Callee::Builtin(Builtin::BitsOf32) => {
                        let x = args[0];
                        let raw = self.builder.ins().bitcast(types::I32, MemFlags::new(), x);
                        let nan = self.builder.ins().fcmp(FloatCC::Unordered, x, x);
                        let canonical = self.builder.ins().iconst(types::I32, CANONICAL_NAN_32);
                        let bits = self.builder.ins().select(nan, canonical, raw);
                        vec![self.builder.ins().uextend(types::I64, bits)]
                    }
                    Callee::Builtin(Builtin::F32OfBits) => {
                        let low = self.builder.ins().ireduce(types::I32, args[0]);
                        vec![self.builder.ins().bitcast(types::F32, MemFlags::new(), low)]
                    }
                    // `docs/f32.md` §2: `sqrt` at binary32 -- `sqrtss` or
                    // `fsqrt s`, one instruction, correctly rounded.
                    Callee::Builtin(Builtin::Sqrt32) => {
                        vec![self.builder.ins().sqrt(args[0])]
                    }
                    // `FloatOf`'s rule at binary32: round to nearest even,
                    // no check, every `int` has a nearest `f32`. Converted
                    // directly from the integer, not through binary64
                    // (which would round twice).
                    Callee::Builtin(Builtin::F32OfInt) => {
                        vec![self.builder.ins().fcvt_from_sint(types::F32, args[0])]
                    }
                    // `Truncate`'s rule (`floating-point.md` §4) at the
                    // narrower width: toward zero, a trap on NaN, infinity
                    // and magnitude at or past `2^63`.
                    // `-2^63` is checked explicitly, as for `Truncate`.
                    Callee::Builtin(Builtin::IntOfF32) => {
                        let x = args[0];
                        let low = self.builder.ins().f32const(-9_223_372_036_854_775_808.0_f32);
                        let too_low = self.builder.ins().fcmp(FloatCC::LessThanOrEqual, x, low);
                        self.builder.ins().trapnz(too_low, TrapCode::INTEGER_OVERFLOW);
                        vec![self.builder.ins().fcvt_to_sint(types::I64, x)]
                    }
                    // `docs/value-barrier.md` §3: the identity. Cranelift
                    // never turns a select or an `and` into a branch, so
                    // there is nothing here for the barrier to stop.
                    Callee::Builtin(Builtin::ValueBarrier) => vec![args[0]],
                    // `docs/crypto-builtins.md` §5: Cranelift emits none of
                    // the AES or carry-less-multiply instructions, so the CPU
                    // never has them here and the two block builtins, which a
                    // program reaches only after `hw_aes_gcm()` answered true,
                    // trap as `trap()` does.
                    Callee::Builtin(Builtin::HwAesGcm) => {
                        vec![self.builder.ins().iconst(types::I8, 0)]
                    }
                    Callee::Builtin(Builtin::AesEncryptBlock | Builtin::GhashUpdate) => {
                        self.builder.ins().trap(TrapCode::unwrap_user(1));
                        let dead = self.builder.create_block();
                        self.builder.switch_to_block(dead);
                        self.builder.seal_block(dead);
                        vec![self.builder.ins().iconst(types::I64, 0)]
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
                    Callee::Builtin(
                        Builtin::OpenRead
                        | Builtin::OpenAppend
                        | Builtin::OpenWrite
                        | Builtin::OpenNew
                        | Builtin::OpenRw
                        | Builtin::OpenDir,
                    ) => {
                        unreachable!("an `open_*` is lowered as `Expr::OpenFile`")
                    }
                    // `docs/file-writes.md` section 4: one libc call each.
                    Callee::Builtin(Builtin::FileWrite) => self.file_write(&args),
                    Callee::Builtin(Builtin::FilePwrite) => self.file_pwrite(&args),
                    Callee::Builtin(Builtin::FilePread) => self.file_pread(&args),
                    Callee::Builtin(Builtin::FileSync) => self.file_sync(&args),
                    Callee::Builtin(Builtin::FileTruncate) => self.file_truncate(&args),
                    Callee::Builtin(Builtin::FileSize) => self.file_size(&args),
                    Callee::Builtin(Builtin::FileLock) => self.file_lock(&args),
                    Callee::Builtin(Builtin::FsRename | Builtin::FsRemove) => {
                        unreachable!("`fs_rename` and `fs_remove` are lowered as `Expr::PathOp`")
                    }
                    // Like the two above: the bound travels with its own
                    // node (`docs/net.md` §4.1).
                    Callee::Builtin(Builtin::Connect) => {
                        unreachable!("`connect` is lowered as `Expr::Connect`")
                    }
                    Callee::Builtin(Builtin::Bind) => {
                        unreachable!("`bind` is lowered as `Expr::Bind`")
                    }
                    Callee::Builtin(Builtin::TcpListen) => {
                        unreachable!("`tcp_listen` is lowered as `Expr::TcpListen`")
                    }
                    Callee::Builtin(Builtin::TcpConnect | Builtin::TcpConnectStart) => {
                        unreachable!("`tcp_connect` is lowered as `Expr::TcpConnect`")
                    }
                    // `docs/native-sockets.md` §3. A borrowed handle arrives
                    // as its address, a buffer as pointer and length, an
                    // owned handle as the descriptor.
                    Callee::Builtin(Builtin::TcpAccept) => self.tcp_accept(&args),
                    Callee::Builtin(Builtin::ConnRead) => self.conn_read(&args),
                    Callee::Builtin(Builtin::ConnWrite) => self.conn_write(&args),
                    // `docs/udp.md` §3: a datagram socket is sent on as a `Conn`
                    // is -- `send(2)` on a connected socket, whole or not at all.
                    Callee::Builtin(Builtin::UdpConnect) => {
                        unreachable!("`udp_connect` is lowered as `Expr::TcpConnect`")
                    }
                    Callee::Builtin(Builtin::UdpBind) => {
                        unreachable!("`udp_bind` is lowered as `Expr::TcpListen`")
                    }
                    Callee::Builtin(Builtin::UdpSend) => self.conn_write(&args),
                    Callee::Builtin(Builtin::UdpRecvFrom) => self.udp_recv_from(&args),
                    Callee::Builtin(Builtin::UdpSendTo) => self.udp_send_to(&args),
                    Callee::Builtin(Builtin::UdpRecv) => self.udp_recv(&args),
                    Callee::Builtin(Builtin::UdpLocalPort) => self.udp_local_port(&args),
                    Callee::Builtin(Builtin::UdpPeer) => self.udp_peer(&args),
                    Callee::Builtin(Builtin::UdpNonblocking) => self.nonblocking(&args),
                    Callee::Builtin(Builtin::PollerAddUdp) => self.poller_ctl(&args, false, false),
                    // `docs/processes.md`: a channel is a socket pair, so its
                    // verbs are the socket handles' (§4.4).
                    Callee::Builtin(Builtin::PipeRead) => self.conn_read(&args),
                    Callee::Builtin(Builtin::PipeWrite) => self.conn_write(&args),
                    Callee::Builtin(Builtin::PipeNonblocking) => self.nonblocking(&args),
                    Callee::Builtin(Builtin::PipeOpen) => self.pipe_open(),
                    Callee::Builtin(Builtin::ChildWait) => self.child_wait(&args),
                    Callee::Builtin(Builtin::ChildKill) => self.child_kill(&args),
                    Callee::Builtin(Builtin::ExecSpawn | Builtin::ExecSpawnIn) => {
                        unreachable!("`exec_spawn` is lowered as `Expr::ExecSpawn`")
                    }
                    // `docs/memory-moves.md`: a bounds-checked `memmove` inside one slice.
                    Callee::Builtin(Builtin::CopyWithin) => self.copy_within(&args),
                    Callee::Builtin(Builtin::CopyInto) => self.copy_into(&args),
                    Callee::Builtin(Builtin::IndexOfByte) => self.index_of_byte(&args),
                    // `docs/native-sockets.md` §4: the poller.
                    Callee::Builtin(Builtin::PollerNew) => self.poller_new(),
                    Callee::Builtin(Builtin::ClockMs) => self.clock_ms(false),
                    Callee::Builtin(Builtin::ClockUnixMs) => self.clock_ms(true),
                    // `docs/signals.md` section 5: the claim.
                    Callee::Builtin(Builtin::SignalsWatch) => self.signals_watch(&args),
                    Callee::Builtin(Builtin::SignalsPending) => self.signals_pending(&args),
                    Callee::Builtin(Builtin::SignalsClose) => self.signals_close(&args),
                    Callee::Builtin(Builtin::PollerAddSignals) => {
                        self.poller_ctl(&args, true, false)
                    }
                    Callee::Builtin(Builtin::ConnDetach) => self.conn_detach(&args, false),
                    Callee::Builtin(Builtin::ConnAttach) => self.conn_attach(&args, false),
                    Callee::Builtin(Builtin::UdpDetach) => self.conn_detach(&args, true),
                    Callee::Builtin(Builtin::UdpAttach) => self.conn_attach(&args, true),
                    Callee::Builtin(Builtin::PollerAddListener) => {
                        self.poller_ctl(&args, true, false)
                    }
                    Callee::Builtin(Builtin::PollerAddConn) => self.poller_ctl(&args, false, false),
                    // `docs/processes.md` §4.8: a channel is watched as a `Conn`
                    // is, and a child by its exit.
                    Callee::Builtin(Builtin::PollerAddPipe) => self.poller_ctl(&args, false, false),
                    Callee::Builtin(Builtin::PollerAddChild) => self.poller_add_child(&args),
                    Callee::Builtin(Builtin::PollerModify) => self.poller_ctl(&args, false, true),
                    Callee::Builtin(Builtin::PollerRemove) => self.poller_remove(&args),
                    Callee::Builtin(Builtin::PollerWait) => self.poller_wait(&args),
                    Callee::Builtin(Builtin::ConnNonblocking | Builtin::ListenerNonblocking) => {
                        self.nonblocking(&args)
                    }
                    Callee::Builtin(Builtin::ConnNodelay) => self.nodelay(&args),
                    Callee::Builtin(Builtin::ConnPeer) => self.conn_peer(&args),
                    Callee::Builtin(Builtin::ConnConnectStatus) => self.connect_status(&args),
                    Callee::Builtin(Builtin::TtyOpen) => {
                        unreachable!("`tty_open` is lowered as `Expr::TtyOpen`")
                    }
                    Callee::Builtin(Builtin::TtyConfigure) => self.tty_configure(&args),
                    Callee::Builtin(Builtin::TtyRead) => self.tty_read(&args),
                    Callee::Builtin(Builtin::TtyWrite) => self.tty_write(&args),
                    Callee::Builtin(Builtin::TtyFlushInput) => self.tty_flush_input(&args),
                    Callee::Builtin(Builtin::PollerAddTty) => self.poller_ctl(&args, false, false),
                    Callee::Builtin(
                        Builtin::ConnClose
                        | Builtin::UdpClose
                        | Builtin::TtyClose
                        | Builtin::ListenerClose
                        | Builtin::PollerClose
                        | Builtin::PipeClose
                        | Builtin::ChildEndClose,
                    ) => {
                        let close = self.libc_fn("close", &[types::I32], &[types::I32]);
                        let close = self.module.declare_func_in_func(close, self.builder.func);
                        let fd = self.builder.ins().ireduce(types::I32, args[0]);
                        let call = self.builder.ins().call(close, &[fd]);
                        let answer = self.builder.inst_results(call)[0];
                        vec![self.builder.ins().sextend(types::I64, answer)]
                    }
                    // `R`, `join`'s real return type, travels with its
                    // own node the same reason the three above do.
                    Callee::Builtin(Builtin::Join) => {
                        unreachable!("`join` is lowered as `Expr::Joined`")
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
                    // Close-on-exec, as every descriptor a builtin makes
                    // (`docs/processes.md` §4.5).
                    Callee::Builtin(Builtin::Accept) => {
                        let fd = self.builder.ins().ireduce(types::I32, args[0]);
                        let answer = self.accept_cloexec(fd);
                        vec![self.builder.ins().sextend(types::I64, answer)]
                    }
                    // `docs/file-handles.md` §3. `read(2)` answers a count,
                    // zero at the end, and `-1` with the reason in `errno` --
                    // which is exactly the three outcomes `Read` has
                    // constructors for, so this is the one place the
                    // sentinel is unpacked and the last.
                    Callee::Builtin(Builtin::ReadFile) => self.read_file(&args),
                    // `docs/directory-handles.md` §2.
                    Callee::Builtin(
                        op @ (Builtin::DirEnter
                        | Builtin::DirOpenRead
                        | Builtin::DirOpenNew
                        | Builtin::DirOpenAppend),
                    ) => self.dir_open(&args, *op),
                    Callee::Builtin(Builtin::DirRename) => self.dir_rename(&args),
                    Callee::Builtin(Builtin::DirRenameNew) => self.dir_rename_new(&args),
                    Callee::Builtin(Builtin::DirRemove) => self.dir_remove(&args),
                    Callee::Builtin(Builtin::DirSync) => self.dir_sync(&args),
                    Callee::Builtin(Builtin::DirClose) => self.dir_close(&args),
                    // `docs/directory-listing.md` §3.1.
                    Callee::Builtin(Builtin::DirList) => self.dir_list(&args),
                    Callee::Builtin(Builtin::DirNext) => self.dir_next(&args),
                    Callee::Builtin(Builtin::DirListClose) => self.dir_list_close(&args),
                    Callee::Builtin(Builtin::DirStat) => self.dir_stat(&args),
                    Callee::Builtin(Builtin::DirMode) => self.dir_mode(&args),
                    Callee::Builtin(Builtin::DirOwnMode) => self.dir_own_mode(&args),
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
                    // `docs/checked-output.md`: the stream the two arms
                    // above write into, flushed and asked whether any of it
                    // failed.
                    Callee::Builtin(Builtin::FlushOut) => self.flush_out(),
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
            // `docs/function-values.md` §4.2: the target's own address,
            // taken rather than called -- the same `FuncId` `Callee::Fn`
            // above declares into this function, read here as a value
            // instead of called.
            Expr::FnValue(id) => {
                let f = self
                    .module
                    .declare_func_in_func(self.declared[id.0 as usize], self.builder.func);
                vec![self.builder.ins().func_addr(self.pointer, f)]
            }
            // A call through a value: `params`/`ret` build the callee's
            // signature the same way `emit.rs`'s own declare loop builds
            // one for every named function, since there is no
            // declaration to read one from at an indirect call site.
            Expr::CallIndirect { target, args, params, ret } => {
                let addr = self.scalar(target);
                let flat_args: Vec<Value> = args.iter().flat_map(|a| self.expr(a)).collect();
                let mut sig = self.module.make_signature();
                sig.call_conv = self.module.isa().default_call_conv();
                for param in params {
                    for leaf in leaves(param, self.program, self.pointer) {
                        sig.params.push(AbiParam::new(leaf));
                    }
                }
                let indirect = returns_indirectly(ret, self.program, self.pointer);
                if indirect {
                    sig.params.insert(0, AbiParam::new(self.pointer));
                } else {
                    for leaf in leaves(ret, self.program, self.pointer) {
                        sig.returns.push(AbiParam::new(leaf));
                    }
                }
                let sig_ref = self.builder.import_signature(sig);
                if !indirect {
                    let call = self.builder.ins().call_indirect(sig_ref, addr, &flat_args);
                    return self.builder.inst_results(call).to_vec();
                }
                // Too wide for registers: the same out-buffer convention
                // `Callee::Fn`'s own wide-return branch uses.
                let buffer = self.return_buffer(ret);
                let mut with_buffer = Vec::with_capacity(flat_args.len() + 1);
                with_buffer.push(buffer);
                with_buffer.extend(flat_args);
                self.builder.ins().call_indirect(sig_ref, addr, &with_buffer);
                let kinds = leaves(ret, self.program, self.pointer);
                self.load_leaves(buffer, &kinds)
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
        // `docs/f32.md` §2: the same instructions at binary32 width. The
        // operands' own type picks the width, so no operator knows one.
        if matches!(self.builder.func.dfg.value_type(a), types::F64 | types::F32) {
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
