//! Expression evaluation: every `Expr` variant, string literals,
//! and every call (builtins and ordinary function calls alike).

use crate::*;

impl<'a> FuncEmitter<'a> {
    pub(crate) fn expr(&mut self, expr: &Expr) -> Result<Vec<LValue>, String> {
        match expr {
            Expr::Int(v) => Ok(vec![LValue::Const(*v)]),
            Expr::Bool(v) => Ok(vec![LValue::Const(i64::from(*v))]),
            // §1: stored as bits already, so this is the one literal
            // here with no decimal round-trip to get wrong.
            Expr::Float(bits) => Ok(vec![LValue::FConst(*bits)]),
            Expr::F32(bits) => Ok(vec![LValue::F32Const(*bits)]),
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
                let (offset, kinds) = self.field_offset(*def, args, *index)?;
                let start = (offset / 8) as usize;
                Ok(values[start..start + kinds.len()].to_vec())
            }
            // `base.index`, where `base` is a *reference* rather than a
            // value (§7.11): the same field arithmetic `Expr::Field`
            // already does, except the leaves are loaded out of the
            // buffer the reference points at instead of picked out of
            // leaves already in registers.
            Expr::FieldRef { base, def, args, index } => {
                let address = self.scalar(base)?;
                let (offset, kinds) = self.field_offset(*def, args, *index)?;
                let addr = self.fresh();
                self.out.push_str(&format!(
                    "  {addr} = getelementptr i8, ptr {}, i64 {offset}\n",
                    operand(&address)
                ));
                Ok(self.load_leaves(&addr, &kinds))
            }
            // `base.index` where the field is `res`: a reference *to*
            // the field rather than a copy of it (`docs/reading-
            // references.md` §2.0) -- the same arithmetic as `Expr::
            // FieldRef`, stopping one step earlier, since that node's
            // own load is exactly what a `res` field cannot survive.
            Expr::FieldAddr { base, def, args, index } => {
                let address = self.scalar(base)?;
                let (offset, _) = self.field_offset(*def, args, *index)?;
                let addr = self.fresh();
                self.out.push_str(&format!(
                    "  {addr} = getelementptr i8, ptr {}, i64 {offset}\n",
                    operand(&address)
                ));
                Ok(vec![LValue::Reg(addr)])
            }
            // `*r` -- the leaves at the address, loaded (`docs/reading-
            // references.md` §3). The same arithmetic a field access
            // does, over the whole referent rather than one member.
            Expr::Deref { ty, value } => {
                let address = self.scalar(value)?;
                let kinds = leaves_of(ty, self.program)?;
                Ok(self.load_leaves(&operand(&address), &kinds))
            }
            // A tuple value (`docs/tuples.md`). Positional already, so
            // unlike `Expr::Struct` there is no declaration order to
            // reorder into -- but the leaves are the same concatenation.
            Expr::Tuple { parts } => {
                let mut out = Vec::new();
                for part in parts {
                    out.extend(self.expr(part)?);
                }
                Ok(out)
            }
            // `base.index` on a tuple: `Expr::Field`'s own arithmetic,
            // with the component types travelling on the node instead of
            // a `DefId` to look them up with.
            Expr::TupleField { base, components, index } => {
                let values = self.expr(base)?;
                let (offset, kinds) = self.tuple_field_offset(components, *index)?;
                let start = (offset / 8) as usize;
                Ok(values[start..start + kinds.len()].to_vec())
            }
            // `Expr::FieldRef`'s counterpart for a type with no
            // declaration.
            Expr::TupleFieldRef { base, components, index } => {
                let address = self.scalar(base)?;
                let (offset, kinds) = self.tuple_field_offset(components, *index)?;
                let addr = self.fresh();
                self.out.push_str(&format!(
                    "  {addr} = getelementptr i8, ptr {}, i64 {offset}\n",
                    operand(&address)
                ));
                Ok(self.load_leaves(&addr, &kinds))
            }
            // `Expr::FieldAddr`'s counterpart for a type with no
            // declaration.
            Expr::TupleFieldAddr { base, components, index } => {
                let address = self.scalar(base)?;
                let (offset, _) = self.tuple_field_offset(components, *index)?;
                let addr = self.fresh();
                self.out.push_str(&format!(
                    "  {addr} = getelementptr i8, ptr {}, i64 {offset}\n",
                    operand(&address)
                ));
                Ok(vec![LValue::Reg(addr)])
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
            // never computed, exactly as `cancho-codegen`'s own
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
            Expr::Subslice { base, start, end, element } => {
                self.subslice(base, start, end, element)
            }
            Expr::AllocSlice { arena, element, count, fill } => {
                self.alloc_slice(*arena, element, count, fill)
            }
            Expr::BoxedSlice { element, count, fill } => self.boxed_slice(element, count, fill),
            // §7.15: single-value allocation, arena or heap -- the same
            // `bump`/`malloc` this backend already opened for a slice's
            // many elements, minus the fill loop.
            Expr::Alloc { arena, ty, value } => Ok(vec![self.alloc(*arena, ty, value)?]),
            Expr::Boxed { ty, value } => Ok(vec![self.boxed(ty, value)?]),
            Expr::Unboxed { ty, value } => self.unboxed(ty, value),
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
            // the negation -- `cancho-codegen`'s own `Expr::Not` arm
            // (`body/expr.rs`), one instruction and never trapping.
            Expr::Not(inner) => {
                let v = self.scalar(inner)?;
                let flipped = self.fresh();
                self.out.push_str(&format!("  {flipped} = xor i8 {}, 1\n", operand(&v)));
                Ok(vec![LValue::Reg(flipped)])
            }
            // `~x` (§7.25, `docs/bitwise.md` §1): every bit, which is the
            // whole difference from `Not` above -- that one knows its
            // operand is 0 or 1 and this one does not, and `bitwise.md`
            // §1 restricts the operator to `int`, so the width is always
            // `i64` and never the `i8` `Not`'s own xor uses.
            Expr::BitNot(inner) => {
                let v = self.scalar(inner)?;
                let flipped = self.fresh();
                self.out.push_str(&format!("  {flipped} = xor i64 {}, -1\n", operand(&v)));
                Ok(vec![LValue::Reg(flipped)])
            }
            // `Expr::Static` (§7.25, `docs/compile-time-data.md` §2): the
            // data was already evaluated and laid out as one read-only
            // global per static in `emit_module` (`emit.rs`), named the
            // same way there and here -- so a read is only a reference to
            // an existing symbol, the same shape a string literal's
            // *value* is after `bytes_lit` has declared its global, minus
            // the declaration itself since a static's global already
            // exists once for the whole program rather than once per
            // occurrence.
            Expr::Static(index) => {
                let data = &self.program.statics[*index as usize];
                Ok(vec![
                    LValue::Reg(format!("@lexs_static_{}", data.name)),
                    LValue::Const(data.values.len() as i64),
                ])
            }
            // `docs/threads.md` §2: blocks until the thread `spawn`
            // started returns, then reads `R`'s own leaves back out of
            // the cell `pthread_join` wrote into -- zero, for `()`, or
            // one, since `lower/conc.rs::spawn` already refused
            // anything wider.
            Expr::Joined { handle, ret } => {
                let handle_value = operand(&self.scalar(handle)?);
                let result_slot = self.fresh();
                self.hoist(format!("  {result_slot} = alloca ptr\n"));
                let status = self.fresh();
                self.out.push_str(&format!(
                    "  {status} = call i32 @pthread_join(ptr {handle_value}, ptr {result_slot})\n"
                ));
                let failed = self.fresh();
                self.out.push_str(&format!("  {failed} = icmp ne i32 {status}, 0\n"));
                self.trap_if(&failed)?;
                // The thread has ended: `signals_watch` may be granted again
                // once the last one has (`docs/signals.md` section 3).
                self.count_thread(-1);
                match leaves_of(ret, self.program)?.as_slice() {
                    [] => Ok(Vec::new()),
                    // The same "load whatever kind the type says straight
                    // out of a `ptr`-typed cell" idiom `body/mod.rs`'s own
                    // slot loads already use -- an `alloca` imposes no
                    // type of its own, and on both this project's
                    // little-endian targets a narrower load already
                    // reads exactly the low bits `pthread_join` wrote.
                    [kind] => {
                        let value = self.fresh();
                        self.out.push_str(&format!(
                            "  {value} = load {}, ptr {result_slot}\n",
                            kind.llvm()
                        ));
                        Ok(vec![LValue::Reg(value)])
                    }
                    _ => unreachable!(
                        "`docs/threads.md` §1 restricts a thread's return to at most one leaf"
                    ),
                }
            }
            // `-x`: on `int`, `0 - x`, checked -- `-int::MIN` has no
            // positive counterpart, the one place negation overflows,
            // the same reasoning `cancho-codegen`'s own `Expr::Neg`
            // uses. On `float`, `fneg` is total, flipping the sign bit
            // even on NaN and on zero, where it is what produces `-0.0`
            // (`docs/floating-point.md` §2) -- never a trap the way
            // `int`'s is.
            Expr::Neg(inner) => {
                let v = self.scalar(inner)?;
                let kind = self.scalar_kind(inner)?;
                if matches!(kind, LKind::F64 | LKind::F32) {
                    let result = self.fresh();
                    self.out.push_str(&format!(
                        "  {result} = fneg {} {}\n",
                        kind.llvm(),
                        operand(&v)
                    ));
                    return Ok(vec![LValue::Reg(result)]);
                }
                self.checked_arith("ssub", LValue::Const(0), v)
            }
            // `bind(net, port)` (§7.21, `docs/listen.md` §6): the second
            // of `Net`'s four builtins this backend lowers, mirroring
            // `cancho-codegen`'s own `Expr::Bind` arm (`body/net.rs`).
            Expr::Bind { bound, args } => {
                let (bound, args) = (bound.clone(), args.clone());
                self.bind(&bound, &args)
            }
            // `tcp_listen` (`docs/native-sockets.md` §3): `bind`'s node with
            // a handle for an answer.
            Expr::TcpListen { bound, args, datagram } => {
                let (bound, args, datagram) = (bound.clone(), args.clone(), *datagram);
                self.tcp_listen(&bound, &args, datagram)
            }
            Expr::TcpConnect { bound, args, start, datagram } => {
                let (bound, args, start, datagram) =
                    (bound.clone(), args.clone(), *start, *datagram);
                self.tcp_connect(&bound, &args, start, datagram)
            }
            // `connect(net, name, port)` (§7.22, `docs/connect.md` §10):
            // the last of `Net`'s four builtins, mirroring
            // `cancho-codegen`'s own `Expr::Connect` arm (`body/net.rs`).
            Expr::Connect { bound, args } => {
                let (bound, args) = (bound.clone(), args.clone());
                self.connect(&bound, &args)
            }
            // `fs_read`/`fs_write` (§7.24, `docs/filesystem.md` §3):
            // dedicated nodes for the same reason `Connect`/`Bind` are
            // -- the prefix the capability was narrowed to travels with
            // the node, mirroring `cancho-codegen`'s own `Expr::FileOp`
            // arm (`body/memory.rs`).
            Expr::FileOp { write, prefix, args } => {
                let (write, prefix, args) = (*write, prefix.clone(), args.clone());
                self.file_op(write, &prefix, &args)
            }
            // `open_read(fs, path)` (§7.24, `docs/file-handles.md`
            // §2.1), mirroring `cancho-codegen`'s own `Expr::OpenFile`
            // arm.
            Expr::OpenFile { prefix, mode, args } => {
                let (prefix, mode, args) = (prefix.clone(), *mode, args.clone());
                self.open_file(&prefix, mode, &args)
            }
            // `docs/tty.md` §3, edition 8.
            Expr::TtyOpen { prefix, args } => {
                let (prefix, args) = (prefix.clone(), args.clone());
                self.tty_open(&prefix, &args)
            }
            // `docs/file-writes.md` section 7.
            Expr::PathOp { op, prefix, args } => {
                let (op, prefix, args) = (*op, prefix.clone(), args.clone());
                self.path_op(op, &prefix, &args)
            }
            // `docs/processes.md` §3.2.
            Expr::ExecSpawn { prefix, in_dir, args } => {
                let (prefix, in_dir, args) = (prefix.clone(), *in_dir, args.clone());
                self.exec_spawn(&prefix, in_dir, &args)
            }
            // `docs/function-values.md` §4.2: the target's own address,
            // taken rather than called. An LLVM global symbol is already
            // a usable `ptr` constant wherever one is expected -- the
            // same reason `bytes_lit`'s own literal needs no instruction
            // either, and `Expr::Static`'s own arm above reads a global
            // the identical way.
            Expr::FnValue(id) => {
                let target = self.program.func(*id);
                Ok(vec![LValue::Reg(format!("@lexs_{}", target.symbol()))])
            }
            // A call through a value: `params`/`ret` build the callee's
            // signature the same way `emit.rs`'s own declare loop builds
            // one for every named function, since there is no
            // declaration to read one from at an indirect call site.
            Expr::CallIndirect { target, args, params, ret } => {
                self.call_indirect(target, args, params, ret)
            }
        }
        // §7.25 closed the last two -- `Expr::BitNot` and `Expr::Static`,
        // both above, next to `Expr::Not` where the operator table
        // already groups them. No wildcard arm precedes this comment on
        // purpose: `rustc` refuses to compile an inexhaustive match with
        // the fallback gone, so the next `Expr` variant this IR gains is
        // a compile error here rather than a silent runtime refusal, the
        // same guarantee an exhaustive match always gives and a wildcard
        // was quietly throwing away.
    }

    /// A string literal's bytes (§5's fourth slice): one read-only global
    /// per occurrence, and no instruction needed to get its address --
    /// unlike Cranelift's `global_value`, an LLVM global symbol is
    /// already a usable `ptr` constant wherever one is expected. Written
    /// as a plain integer-array constant (`[i8 72, i8 105, ...]`) rather
    /// than the `c"..."` shorthand, which needs its own escaping rules
    /// this backend has no reason to also get right.
    pub(crate) fn bytes_lit(&mut self, text: &str) -> Vec<LValue> {
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

    pub(crate) fn call(&mut self, callee: &Callee, args: &[Expr]) -> Result<Vec<LValue>, String> {
        let evaluated: Vec<Vec<LValue>> =
            args.iter().map(|a| self.expr(a)).collect::<Result<_, _>>()?;

        match callee {
            Callee::Builtin(
                Builtin::Split | Builtin::Narrow | Builtin::ForkHeap | Builtin::ForkClock,
            ) => Ok(Vec::new()),
            Callee::Builtin(Builtin::Release) => Ok(vec![LValue::Const(0)]),
            // `docs/opaque-pointers.md` §3: the null handle. `LKind::Ptr`'s
            // own `zero()` is already the literal LLVM needs here --
            // `null`, not `0`, since a bare integer is not a valid `ptr`
            // operand.
            Callee::Builtin(Builtin::NullPtr) => {
                Ok(vec![LValue::Reg(LKind::Ptr.zero().to_owned())])
            }
            // An explicit, deliberate trap (`docs/testing.md` §2) rather
            // than a check the compiler inserted on its own. `trap_if`
            // already takes an arbitrary condition; a literal `i1 true`
            // makes it unconditional, reusing the exact instruction every
            // checked operation already traps with, on both targets.
            // Nothing after it runs, but the call still has to answer
            // with a value of the right shape.
            Callee::Builtin(Builtin::Trap) => {
                self.trap_if("true")?;
                Ok(vec![LValue::Reg(LKind::I64.zero().to_owned())])
            }
            // `docs/threads.md` §2: `body`'s own compiled entry point
            // (`evaluated[1]`, `Expr::FnValue`'s address) becomes
            // `pthread_create`'s start routine directly -- checked at
            // the call site (`lower/conc.rs::spawn`) to be safe as one,
            // so no trampoline is built here. `Thread[T, R]`'s own
            // single leaf is the raw `pthread_t` `pthread_create` wrote
            // into an `alloca`'d cell this call owns.
            Callee::Builtin(Builtin::Spawn) => {
                // `void *arg` needs a real `ptr` operand: `int`/`bool`
                // cross a call as their own width (`i64`/`i8`), so a
                // non-`ptr` payload is reinterpreted with `inttoptr`
                // first -- the bits are unchanged, only how LLVM's
                // textual IR is allowed to spell them at a call site. A
                // `()` payload (`docs/threads.md` §2, zero leaves) has
                // nothing real to pass; `body`'s own compiled code never
                // reads it either, so `null` is exactly as good as
                // anything else would be.
                let payload = match evaluated[0].as_slice() {
                    [] => "null".to_owned(),
                    [value] => {
                        let kind = self.scalar_kind(&args[0])?;
                        if kind == LKind::Ptr {
                            operand(value)
                        } else {
                            let converted = self.fresh();
                            self.out.push_str(&format!(
                                "  {converted} = inttoptr {} {} to ptr\n",
                                kind.llvm(),
                                operand(value)
                            ));
                            converted
                        }
                    }
                    _ => {
                        unreachable!("`docs/threads.md` §1 restricts a payload to at most one leaf")
                    }
                };
                let start_routine = operand(&evaluated[1][0]);
                let thread_slot = self.fresh();
                self.hoist(format!("  {thread_slot} = alloca ptr\n"));
                let status = self.fresh();
                self.out.push_str(&format!(
                    "  {status} = call i32 @pthread_create(ptr {thread_slot}, ptr null, ptr {start_routine}, ptr {payload})\n"
                ));
                let failed = self.fresh();
                self.out.push_str(&format!("  {failed} = icmp ne i32 {status}, 0\n"));
                self.trap_if(&failed)?;
                // A running thread forbids a new signal claim: it would not
                // have the signals blocked (`docs/signals.md` section 3).
                self.count_thread(1);
                let thread_value = self.fresh();
                self.out.push_str(&format!("  {thread_value} = load ptr, ptr {thread_slot}\n"));
                Ok(vec![LValue::Reg(thread_value)])
            }
            // `R`, `join`'s real return type, travels with its own
            // node, `Expr::Joined`, the same reason a file operation's
            // prefix or `connect`'s bound do.
            Callee::Builtin(Builtin::Join) => {
                unreachable!("`join` is lowered as `Expr::Joined`")
            }
            // The escape from checked arithmetic (`docs/llvm-backend.md`
            // §7.3's first named gap): LLVM's own `add`/`sub`/`mul`, with
            // no `nsw`/`nuw` requested, are already two's-complement
            // wraparound -- `cancho-codegen`'s plain `iadd`/`isub`/`imul`
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
                let putchar = if crate::wasi_console::applies(self.triple) {
                    "cancho_putchar"
                } else {
                    "putchar"
                };
                self.out.push_str(&format!("  {result} = call i32 @{putchar}(i32 {narrowed})\n"));
                let widened = self.fresh();
                self.out.push_str(&format!("  {widened} = sext i32 {result} to i64\n"));
                Ok(vec![LValue::Reg(widened)])
            }
            // `docs/standard-input.md` §3: the mirror of `putchar`, no
            // argument to erase or narrow -- `getchar`'s own `io: &!i Io`
            // is already zero leaves. Sign-extended, not zero-extended:
            // `EOF` is `-1`, and zero-extending would hand the program
            // 4294967295, which is a byte-range check that silently
            // never fires.
            Callee::Builtin(Builtin::GetChar) => {
                let result = self.fresh();
                let getchar = if crate::wasi_console::applies(self.triple) {
                    "cancho_getchar"
                } else {
                    "getchar"
                };
                self.out.push_str(&format!("  {result} = call i32 @{getchar}()\n"));
                let widened = self.fresh();
                self.out.push_str(&format!("  {widened} = sext i32 {result} to i64\n"));
                Ok(vec![LValue::Reg(widened)])
            }
            // `write_bytes`/`write_err` (§7.11): the whole slice in one
            // `fwrite`, through the stream the module header already
            // declared for this platform -- `docs/bulk-io.md` §3 and
            // `docs/standard-error.md` §3.3 are the same call, one
            // symbol apart.
            Callee::Builtin(Builtin::Write | Builtin::WriteErr) => {
                let skip = Builtin::Write.erased_args();
                let flat: Vec<LValue> = evaluated.into_iter().skip(skip).flatten().collect();
                let [start, len] = flat.as_slice() else {
                    return Err("`write_bytes`/`write_err` need a byte slice argument".to_owned());
                };
                if crate::wasi_console::applies(self.triple) {
                    // The console without libc's stdio (`wasi_console`): the same
                    // bytes, through `fd_write` alone.
                    let write = if matches!(callee, Callee::Builtin(Builtin::WriteErr)) {
                        "cancho_stderr_write"
                    } else {
                        "cancho_stdout_write"
                    };
                    let st = self.size_ty();
                    let size = self.size_arg(&operand(len));
                    let raw = self.fresh();
                    self.out.push_str(&format!(
                        "  {raw} = call {st} @{write}(ptr {}, {st} {size})\n",
                        operand(start)
                    ));
                    let result = self.size_result(&raw, false);
                    return Ok(vec![LValue::Reg(result)]);
                }
                let symbol = match (callee, self.triple.operating_system) {
                    (
                        Callee::Builtin(Builtin::WriteErr),
                        target_lexicon::OperatingSystem::Darwin(_),
                    ) => "__stderrp",
                    (Callee::Builtin(Builtin::WriteErr), _) => "stderr",
                    (_, target_lexicon::OperatingSystem::Darwin(_)) => "__stdoutp",
                    (_, _) => "stdout",
                };
                let stream = self.fresh();
                self.out.push_str(&format!("  {stream} = load ptr, ptr @{symbol}\n"));
                let st = self.size_ty();
                let size = self.size_arg(&operand(len));
                let raw = self.fresh();
                self.out.push_str(&format!(
                    "  {raw} = call {st} @fwrite(ptr {}, {st} 1, {st} {size}, ptr {stream})\n",
                    operand(start)
                ));
                let result = self.size_result(&raw, false);
                Ok(vec![LValue::Reg(result)])
            }
            // `docs/checked-output.md`: the stream the arm above writes
            // into, flushed and asked whether any of it failed.
            Callee::Builtin(Builtin::FlushOut) => Ok(self.flush_out()),
            // `docs/arguments.md` §3: `argc`, exactly as `main` was
            // handed it and stashed into `@lexs_argc` before this
            // function's own body could run.
            Callee::Builtin(Builtin::ArgCount) => {
                let count = self.fresh();
                self.out.push_str(&format!("  {count} = load i64, ptr @lexs_argc\n"));
                Ok(vec![LValue::Reg(count)])
            }
            // One argument, as a pointer and a length (§3, same section):
            // an index outside `0 .. argc` traps, the same mistake and
            // the same answer as indexing past a slice, then `argv[n]` is
            // read back and its NUL-terminated length computed with
            // `strlen` -- the terminator is an artifact of the C
            // interface, not part of the value handed back.
            Callee::Builtin(Builtin::Arg) => {
                let skip = Builtin::Arg.erased_args();
                let index = evaluated
                    .into_iter()
                    .skip(skip)
                    .flatten()
                    .next()
                    .ok_or_else(|| "`arg` needs an index argument".to_owned())?;
                let count = self.fresh();
                self.out.push_str(&format!("  {count} = load i64, ptr @lexs_argc\n"));
                let past = self.fresh();
                self.out
                    .push_str(&format!("  {past} = icmp uge i64 {}, {count}\n", operand(&index)));
                self.trap_if(&past)?;
                let argv = self.fresh();
                self.out.push_str(&format!("  {argv} = load ptr, ptr @lexs_argv\n"));
                let slot = self.fresh();
                self.out.push_str(&format!(
                    "  {slot} = getelementptr ptr, ptr {argv}, i64 {}\n",
                    operand(&index)
                ));
                let text = self.fresh();
                self.out.push_str(&format!("  {text} = load ptr, ptr {slot}\n"));
                let st = self.size_ty();
                let raw = self.fresh();
                self.out.push_str(&format!("  {raw} = call {st} @strlen(ptr {text})\n"));
                let length = self.size_result(&raw, false);
                Ok(vec![LValue::Reg(text), LValue::Reg(length)])
            }
            // `int_of(b: byte) -> int` widens, always defined: every
            // `byte` is 0..255, so zero-extension is exact -- the direct
            // counterpart of `cancho-codegen`'s `uextend`.
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
            // `float_of(n: int) -> float` widens, never traps: every
            // `int` has a nearest `float`, round-to-nearest-even beyond
            // `2^53` (`docs/floating-point.md` §4) -- `sitofp` is that
            // rounding exactly, the direct counterpart of Cranelift's
            // `fcvt_from_sint`.
            Callee::Builtin(Builtin::FloatOf) => {
                let n = evaluated
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or_else(|| "`float_of` needs an int argument".to_owned())?;
                let result = self.fresh();
                self.out.push_str(&format!("  {result} = sitofp i64 {} to double\n", operand(&n)));
                Ok(vec![LValue::Reg(result)])
            }
            // `truncate(x: float) -> int`: toward zero, trapping on
            // exactly the inputs C leaves undefined -- NaN, either
            // infinity, and any magnitude at or beyond `2^63` (§4).
            // LLVM's `fptosi` is poison on all of those, unlike
            // Cranelift's `fcvt_to_sint`, which traps in hardware -- so
            // the three checks are explicit here, ahead of the
            // instruction, the same shape `checked_div`'s own is.
            // `llvm.fptosi.sat` is not the answer: saturating is the
            // silently wrong result this project already refuses.
            Callee::Builtin(Builtin::Truncate) => {
                let x = evaluated
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or_else(|| "`truncate` needs a float argument".to_owned())?;
                let x_op = operand(&x);
                let is_nan = self.fresh();
                self.out.push_str(&format!("  {is_nan} = fcmp uno double {x_op}, {x_op}\n"));
                self.trap_if(&is_nan)?;
                let too_high = self.fresh();
                self.out.push_str(&format!(
                    "  {too_high} = fcmp oge double {x_op}, {TRUNCATE_UPPER_BOUND}\n"
                ));
                self.trap_if(&too_high)?;
                let too_low = self.fresh();
                self.out.push_str(&format!(
                    "  {too_low} = fcmp ole double {x_op}, {TRUNCATE_LOWER_BOUND}\n"
                ));
                self.trap_if(&too_low)?;
                let result = self.fresh();
                self.out.push_str(&format!("  {result} = fptosi double {x_op} to i64\n"));
                Ok(vec![LValue::Reg(result)])
            }
            // `bits_of(x: float) -> int`: a reinterpretation, `bitcast`
            // and no arithmetic, except every NaN canonicalises to one
            // pattern (`docs/floating-point.md` §4.1) -- the same
            // `select`-over-a-NaN-test `cancho-codegen`'s own `BitsOf`
            // already does.
            // `docs/f32.md` §2: `sqrt` at binary32, one intrinsic, correctly
            // rounded -- what Cranelift's bare `sqrt` is on an `F32`.
            Callee::Builtin(Builtin::Sqrt32) => {
                let x = evaluated
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or_else(|| "`sqrt32` needs an f32 argument".to_owned())?;
                let result = self.fresh();
                self.out.push_str(&format!(
                    "  {result} = call float @llvm.sqrt.f32(float {})\n",
                    operand(&x)
                ));
                Ok(vec![LValue::Reg(result)])
            }
            // `f32_of_int`: `sitofp i64 to float` rounds once, to nearest
            // even, straight from the integer (not through `double`, which
            // would round twice) -- Cranelift's `fcvt_from_sint` on F32.
            Callee::Builtin(Builtin::F32OfInt) => {
                let n = evaluated
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or_else(|| "`f32_of_int` needs an int argument".to_owned())?;
                let result = self.fresh();
                self.out.push_str(&format!("  {result} = sitofp i64 {} to float\n", operand(&n)));
                Ok(vec![LValue::Reg(result)])
            }
            // `int_of_f32`: `truncate`'s three explicit checks at the
            // narrower width (`fptosi` is poison on exactly those inputs),
            // then `fptosi float to i64`. `2^63` is the same constant: it
            // is a power of two, so exact in binary32.
            Callee::Builtin(Builtin::IntOfF32) => {
                let x = evaluated
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or_else(|| "`int_of_f32` needs an f32 argument".to_owned())?;
                let x_op = operand(&x);
                let is_nan = self.fresh();
                self.out.push_str(&format!("  {is_nan} = fcmp uno float {x_op}, {x_op}\n"));
                self.trap_if(&is_nan)?;
                let too_high = self.fresh();
                self.out.push_str(&format!(
                    "  {too_high} = fcmp oge float {x_op}, {TRUNCATE_UPPER_BOUND}\n"
                ));
                self.trap_if(&too_high)?;
                let too_low = self.fresh();
                self.out.push_str(&format!(
                    "  {too_low} = fcmp ole float {x_op}, {TRUNCATE_LOWER_BOUND}\n"
                ));
                self.trap_if(&too_low)?;
                let result = self.fresh();
                self.out.push_str(&format!("  {result} = fptosi float {x_op} to i64\n"));
                Ok(vec![LValue::Reg(result)])
            }
            // `docs/value-barrier.md` §3: an empty `asm` whose output is
            // tied to its input. It emits no instruction, and LLVM knows
            // nothing about its answer, so a mask passed through it stays
            // an `and` rather than becoming a branch on the secret.
            Callee::Builtin(Builtin::ValueBarrier) => {
                let x = evaluated
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or_else(|| "`value_barrier` needs an int argument".to_owned())?;
                let x_op = operand(&x);
                let result = self.fresh();
                self.out
                    .push_str(&format!("  {result} = call i64 asm \"\", \"=r,0\"(i64 {x_op})\n"));
                Ok(vec![LValue::Reg(result)])
            }
            // `docs/f32.md` §2: `fptrunc` is IEEE conversion to the
            // narrower format under the default rounding mode
            // (round-to-nearest-even), an overflow is infinity, and
            // `fpext` is exact -- the counterparts of Cranelift's
            // `fdemote` and `fpromote`.
            Callee::Builtin(Builtin::F32Of) => {
                let x = evaluated
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or_else(|| "`f32_of` needs a float argument".to_owned())?;
                let result = self.fresh();
                self.out
                    .push_str(&format!("  {result} = fptrunc double {} to float\n", operand(&x)));
                Ok(vec![LValue::Reg(result)])
            }
            Callee::Builtin(Builtin::FloatOf32) => {
                let x = evaluated
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or_else(|| "`float_of32` needs an f32 argument".to_owned())?;
                let result = self.fresh();
                self.out.push_str(&format!("  {result} = fpext float {} to double\n", operand(&x)));
                Ok(vec![LValue::Reg(result)])
            }
            // As `BitsOf`, at 32 bits, zero-extended.
            Callee::Builtin(Builtin::BitsOf32) => {
                let x = evaluated
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or_else(|| "`bits_of32` needs an f32 argument".to_owned())?;
                let x_op = operand(&x);
                let raw = self.fresh();
                self.out.push_str(&format!("  {raw} = bitcast float {x_op} to i32\n"));
                let is_nan = self.fresh();
                self.out.push_str(&format!("  {is_nan} = fcmp uno float {x_op}, {x_op}\n"));
                let bits = self.fresh();
                self.out.push_str(&format!(
                    "  {bits} = select i1 {is_nan}, i32 {CANONICAL_NAN_32}, i32 {raw}\n"
                ));
                let result = self.fresh();
                self.out.push_str(&format!("  {result} = zext i32 {bits} to i64\n"));
                Ok(vec![LValue::Reg(result)])
            }
            Callee::Builtin(Builtin::F32OfBits) => {
                let n = evaluated
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or_else(|| "`f32_of_bits` needs an int argument".to_owned())?;
                let low = self.fresh();
                self.out.push_str(&format!("  {low} = trunc i64 {} to i32\n", operand(&n)));
                let result = self.fresh();
                self.out.push_str(&format!("  {result} = bitcast i32 {low} to float\n"));
                Ok(vec![LValue::Reg(result)])
            }
            Callee::Builtin(Builtin::FloatOfBits) => {
                let n = evaluated
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or_else(|| "`float_of_bits` needs an int argument".to_owned())?;
                let result = self.fresh();
                self.out.push_str(&format!("  {result} = bitcast i64 {} to double\n", operand(&n)));
                Ok(vec![LValue::Reg(result)])
            }
            Callee::Builtin(Builtin::BitsOf) => {
                let x = evaluated
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or_else(|| "`bits_of` needs a float argument".to_owned())?;
                let x_op = operand(&x);
                let raw = self.fresh();
                self.out.push_str(&format!("  {raw} = bitcast double {x_op} to i64\n"));
                let is_nan = self.fresh();
                self.out.push_str(&format!("  {is_nan} = fcmp uno double {x_op}, {x_op}\n"));
                let result = self.fresh();
                self.out.push_str(&format!(
                    "  {result} = select i1 {is_nan}, i64 {CANONICAL_NAN}, i64 {raw}\n"
                ));
                Ok(vec![LValue::Reg(result)])
            }
            // `is_nan(x)`: `x != x`, true for NaN and nothing else --
            // the riddle `cancho-codegen`'s own `IsNan` is named for.
            // `une` (unordered-or-not-equal), not the ordered `one`,
            // the same predicate `!=` itself uses in `float_compare`.
            Callee::Builtin(Builtin::IsNan) => {
                let x = evaluated
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or_else(|| "`is_nan` needs a float argument".to_owned())?;
                let x_op = operand(&x);
                let nan = self.fresh();
                self.out.push_str(&format!("  {nan} = fcmp une double {x_op}, {x_op}\n"));
                let widened = self.fresh();
                self.out.push_str(&format!("  {widened} = zext i1 {nan} to i8\n"));
                Ok(vec![LValue::Reg(widened)])
            }
            // `sqrt`: one instruction, correctly rounded per IEEE-754 --
            // the same reason `docs/float-math.md` §2 says this cannot
            // be library code, matching `cancho-codegen`'s own bare
            // `sqrt` instruction exactly.
            Callee::Builtin(Builtin::Sqrt) => {
                let x = evaluated
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or_else(|| "`sqrt` needs a float argument".to_owned())?;
                let result = self.fresh();
                self.out.push_str(&format!(
                    "  {result} = call double @llvm.sqrt.f64(double {})\n",
                    operand(&x)
                ));
                Ok(vec![LValue::Reg(result)])
            }
            // `listen(fd: int, backlog: int) -> int`: neither takes a
            // capability (`docs/listen.md` §6 -- the port was already
            // bound at `bind`), so this is an ordinary fixed-signature
            // `libc` call, narrowed to `i32` and widened back the same
            // way `cancho-codegen`'s own arm does.
            Callee::Builtin(Builtin::Listen) => {
                let mut args = evaluated.into_iter().flatten();
                let fd = args.next().ok_or_else(|| "`listen` needs an fd argument".to_owned())?;
                let backlog =
                    args.next().ok_or_else(|| "`listen` needs a backlog argument".to_owned())?;
                let fd32 = self.fresh();
                self.out.push_str(&format!("  {fd32} = trunc i64 {} to i32\n", operand(&fd)));
                let backlog32 = self.fresh();
                self.out
                    .push_str(&format!("  {backlog32} = trunc i64 {} to i32\n", operand(&backlog)));
                let result = self.fresh();
                self.out.push_str(&format!(
                    "  {result} = call i32 @listen(i32 {fd32}, i32 {backlog32})\n"
                ));
                let widened = self.fresh();
                self.out.push_str(&format!("  {widened} = sext i32 {result} to i64\n"));
                Ok(vec![LValue::Reg(widened)])
            }
            // `accept(fd: int) -> int`: the peer address is ignored --
            // `NULL, NULL` -- the same as `examples/serve/`'s own
            // hand-written call and Cranelift's arm.
            Callee::Builtin(Builtin::Accept) => {
                let fd = evaluated
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or_else(|| "`accept` needs an fd argument".to_owned())?;
                let fd32 = self.fresh();
                self.out.push_str(&format!("  {fd32} = trunc i64 {} to i32\n", operand(&fd)));
                // Close-on-exec, as every descriptor a builtin makes
                // (`docs/processes.md` §4.5).
                let result = self.accept_cloexec(&fd32);
                let widened = self.fresh();
                self.out.push_str(&format!("  {widened} = sext i32 {result} to i64\n"));
                Ok(vec![LValue::Reg(widened)])
            }
            // `file_read(file, into)` (§7.24, `docs/file-handles.md`
            // §3): `file` is a reference (`&!f File`), so it is not a
            // capability and is not erased -- `args[0]` is its one
            // pointer leaf, `args[1..]` the buffer.
            Callee::Builtin(Builtin::ReadFile) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                if args.len() != 3 {
                    return Err(format!(
                        "`file_read` needs 3 leaves but {} were given",
                        args.len()
                    ));
                }
                self.read_file(&args)
            }
            // `docs/file-writes.md` section 4: one libc call each. The
            // handle arrives as its address, a slice as pointer and length.
            Callee::Builtin(
                op @ (Builtin::FileWrite
                | Builtin::FilePwrite
                | Builtin::FilePread
                | Builtin::FileSync
                | Builtin::FileTruncate
                | Builtin::FileSize
                | Builtin::FileLock),
            ) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                let leaves = match op {
                    Builtin::FileWrite => 3,
                    Builtin::FilePwrite | Builtin::FilePread => 4,
                    Builtin::FileTruncate => 2,
                    _ => 1,
                };
                if args.len() != leaves {
                    return Err(format!(
                        "`{}` needs {leaves} leaves but {} were given",
                        op.name(),
                        args.len()
                    ));
                }
                match op {
                    Builtin::FileWrite => self.file_write(&args),
                    Builtin::FilePwrite => self.file_pwrite(&args),
                    Builtin::FilePread => self.file_pread(&args),
                    Builtin::FileSync => self.file_sync(&args),
                    Builtin::FileTruncate => self.file_truncate(&args),
                    Builtin::FileLock => self.file_lock(&args),
                    _ => self.file_size(&args),
                }
            }
            // `file_close(file)` (§7.24): `file` is `File` by value, one
            // leaf -- the descriptor itself, not its address, unlike
            // `file_read`'s reference above.
            Callee::Builtin(Builtin::Close) => {
                let fd64 = evaluated
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or_else(|| "`file_close` needs a file argument".to_owned())?;
                let fd = self.fresh();
                self.out.push_str(&format!("  {fd} = trunc i64 {} to i32\n", operand(&fd64)));
                let result = self.fresh();
                self.out.push_str(&format!("  {result} = call i32 @close(i32 {fd})\n"));
                let widened = self.fresh();
                self.out.push_str(&format!("  {widened} = sext i32 {result} to i64\n"));
                Ok(vec![LValue::Reg(widened)])
            }
            // `docs/native-sockets.md` §3: the socket handles. A borrowed
            // handle arrives as its address (one leaf), a buffer as a
            // pointer and a length, an owned handle as the descriptor.
            Callee::Builtin(Builtin::TcpAccept) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.tcp_accept(&args)
            }
            Callee::Builtin(Builtin::ConnRead) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                if args.len() != 3 {
                    return Err(format!(
                        "`conn_read` needs 3 leaves but {} were given",
                        args.len()
                    ));
                }
                self.conn_read(&args)
            }
            // `docs/memory-moves.md`: a bounds-checked `memmove` inside one slice.
            Callee::Builtin(Builtin::CopyWithin) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                if args.len() != 5 {
                    return Err(format!(
                        "`copy_within` needs 5 leaves but {} were given",
                        args.len()
                    ));
                }
                self.copy_within(&args)
            }
            // `docs/bulk-copy.md`: one bounds check, then one `memmove` between two slices.
            Callee::Builtin(Builtin::CopyInto) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                if args.len() != 4 {
                    return Err(format!(
                        "`copy_into` needs 4 leaves but {} were given",
                        args.len()
                    ));
                }
                self.copy_into(&args)
            }
            // `docs/crypto-builtins.md` §4: calls of `crate::crypto`'s
            // functions, after the length checks (`body/crypto.rs`).
            Callee::Builtin(Builtin::HwAesGcm) => self.hw_aes_gcm(),
            Callee::Builtin(Builtin::AesEncryptBlock) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.aes_encrypt_block(&args)
            }
            Callee::Builtin(Builtin::GhashUpdate) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.ghash_update(&args)
            }
            // `docs/byte-search.md`: one `memchr`.
            Callee::Builtin(Builtin::IndexOfByte) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                if args.len() != 3 {
                    return Err(format!(
                        "`index_of_byte` needs 3 leaves but {} were given",
                        args.len()
                    ));
                }
                self.index_of_byte(&args)
            }
            // `docs/processes.md`: a channel is a socket pair, so its verbs
            // are the socket handles' (§4.4).
            Callee::Builtin(Builtin::ConnWrite | Builtin::PipeWrite | Builtin::UdpSend) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                if args.len() != 3 {
                    return Err(format!(
                        "`conn_write` needs 3 leaves but {} were given",
                        args.len()
                    ));
                }
                self.conn_write(&args)
            }
            // `docs/udp.md` §3: a datagram socket is received on with its own verb.
            Callee::Builtin(Builtin::UdpRecv) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                if args.len() != 3 {
                    return Err(format!("`udp_recv` needs 3 leaves but {} were given", args.len()));
                }
                self.udp_recv(&args)
            }
            Callee::Builtin(Builtin::UdpRecvFrom) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                if args.len() != 5 {
                    return Err(format!(
                        "`udp_recv_from` needs 5 leaves but {} were given",
                        args.len()
                    ));
                }
                self.udp_recv_from(&args)
            }
            Callee::Builtin(Builtin::UdpSendTo) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                if args.len() != 4 {
                    return Err(format!(
                        "`udp_send_to` needs 4 leaves but {} were given",
                        args.len()
                    ));
                }
                self.udp_send_to(&args)
            }
            Callee::Builtin(Builtin::UdpLocalPort) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.udp_local_port(&args)
            }
            Callee::Builtin(Builtin::UdpPeer) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                if args.len() != 4 {
                    return Err(format!("`udp_peer` needs 4 leaves but {} were given", args.len()));
                }
                self.udp_peer(&args)
            }
            Callee::Builtin(Builtin::PipeRead) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                if args.len() != 3 {
                    return Err(format!(
                        "`pipe_read` needs 3 leaves but {} were given",
                        args.len()
                    ));
                }
                self.conn_read(&args)
            }
            Callee::Builtin(Builtin::PipeOpen) => self.pipe_open(),
            Callee::Builtin(Builtin::ChildWait) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.child_wait(&args)
            }
            Callee::Builtin(Builtin::ChildKill) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.child_kill(&args)
            }
            Callee::Builtin(Builtin::ExecSpawn | Builtin::ExecSpawnIn) => {
                Err("`exec_spawn` is lowered as `Expr::ExecSpawn`".to_owned())
            }
            // `docs/native-sockets.md` §4: the poller.
            Callee::Builtin(Builtin::PollerNew) => self.poller_new(),
            Callee::Builtin(Builtin::ClockMs) => self.clock_ms(false),
            Callee::Builtin(Builtin::ClockUnixMs) => self.clock_ms(true),
            // `docs/directory-handles.md` §2.
            Callee::Builtin(
                op @ (Builtin::DirEnter
                | Builtin::DirOpenRead
                | Builtin::DirOpenNew
                | Builtin::DirOpenAppend),
            ) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.dir_open(&args, *op)
            }
            Callee::Builtin(Builtin::DirRename) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.dir_rename(&args)
            }
            Callee::Builtin(Builtin::DirRenameNew) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.dir_rename_new(&args)
            }
            Callee::Builtin(Builtin::DirRemove) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.dir_remove(&args)
            }
            Callee::Builtin(Builtin::DirSync) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.dir_sync(&args)
            }
            Callee::Builtin(Builtin::DirClose) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.dir_close(&args)
            }
            // `docs/directory-listing.md` §3.1.
            Callee::Builtin(Builtin::DirList) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.dir_list(&args)
            }
            Callee::Builtin(Builtin::DirNext) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.dir_next(&args)
            }
            Callee::Builtin(Builtin::DirListClose) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.dir_list_close(&args)
            }
            Callee::Builtin(Builtin::DirStat) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.dir_stat(&args)
            }
            Callee::Builtin(Builtin::DirMode) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.dir_mode(&args)
            }
            Callee::Builtin(Builtin::DirOwnMode) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.dir_own_mode(&args)
            }
            // `docs/signals.md` section 5: the claim.
            Callee::Builtin(Builtin::SignalsWatch) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.signals_watch(&args)
            }
            Callee::Builtin(Builtin::SignalsPending) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.signals_pending(&args)
            }
            Callee::Builtin(Builtin::SignalsClose) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.signals_close(&args)
            }
            // `docs/processes.md` §4.8: a channel is watched as a `Conn` is,
            // and a child by its exit.
            Callee::Builtin(Builtin::PollerAddPipe) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.poller_ctl(&args, false, false)
            }
            Callee::Builtin(Builtin::PollerAddChild) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.poller_add_child(&args)
            }
            Callee::Builtin(Builtin::PollerAddSignals) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.poller_ctl(&args, true, false)
            }
            Callee::Builtin(Builtin::ConnDetach) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.conn_detach(&args, false)
            }
            Callee::Builtin(Builtin::UdpDetach) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.conn_detach(&args, true)
            }
            Callee::Builtin(Builtin::UdpAttach) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.conn_attach(&args, true)
            }
            Callee::Builtin(Builtin::ConnAttach) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.conn_attach(&args, false)
            }
            // `docs/tty.md` §3, edition 8: configure, read, write, flush,
            // and the poller family's sixth member.
            Callee::Builtin(Builtin::TtyOpen) => {
                unreachable!("`tty_open` is lowered as `Expr::TtyOpen`")
            }
            Callee::Builtin(Builtin::TtyConfigure) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.tty_configure(&args)
            }
            Callee::Builtin(Builtin::TtyRead) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.tty_read(&args)
            }
            Callee::Builtin(Builtin::TtyWrite) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.tty_write(&args)
            }
            Callee::Builtin(Builtin::TtyFlushInput) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.tty_flush_input(&args)
            }
            Callee::Builtin(Builtin::PollerAddTty) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.poller_ctl(&args, false, false)
            }
            Callee::Builtin(Builtin::PollerAddListener) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.poller_ctl(&args, true, false)
            }
            Callee::Builtin(Builtin::PollerAddConn | Builtin::PollerAddUdp) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.poller_ctl(&args, false, false)
            }
            Callee::Builtin(Builtin::PollerModify) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.poller_ctl(&args, false, true)
            }
            Callee::Builtin(Builtin::PollerRemove) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.poller_remove(&args)
            }
            Callee::Builtin(Builtin::PollerWait) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                if args.len() != 4 {
                    return Err(format!(
                        "`poller_wait` needs 4 leaves but {} were given",
                        args.len()
                    ));
                }
                self.poller_wait(&args)
            }
            Callee::Builtin(
                Builtin::ConnNonblocking
                | Builtin::ListenerNonblocking
                | Builtin::PipeNonblocking
                | Builtin::UdpNonblocking,
            ) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.nonblocking(&args)
            }
            Callee::Builtin(Builtin::ConnPeer) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                if args.len() != 3 {
                    return Err(format!(
                        "`conn_peer` needs 3 leaves but {} were given",
                        args.len()
                    ));
                }
                self.conn_peer(&args)
            }
            Callee::Builtin(Builtin::ConnNodelay) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.nodelay(&args)
            }
            Callee::Builtin(Builtin::ConnConnectStatus) => {
                let args: Vec<LValue> = evaluated.into_iter().flatten().collect();
                self.connect_status(&args)
            }
            Callee::Builtin(
                Builtin::ConnClose
                | Builtin::UdpClose
                | Builtin::TtyClose
                | Builtin::ListenerClose
                | Builtin::PollerClose
                | Builtin::PipeClose
                | Builtin::ChildEndClose,
            ) => {
                let fd64 = evaluated
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or_else(|| "a close needs a handle argument".to_owned())?;
                let fd = self.fresh();
                self.out.push_str(&format!("  {fd} = trunc i64 {} to i32\n", operand(&fd64)));
                let result = self.fresh();
                self.out.push_str(&format!("  {result} = call i32 @close(i32 {fd})\n"));
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
                let symbol = format!("lexs_{}", target.symbol());
                Ok(self.emit_call(&symbol, &printed, &ret_kinds))
            }
            Callee::Builtin(other) => Err(format!(
                "`{}` is not part of the LLVM backend's first slice (docs/llvm-backend.md §5)",
                other.name()
            )),
            // `extern fn` (§7.23, §8.4): an import under the symbol the
            // declaration named, `emit.rs`'s own declare loop having
            // already given it a signature there. A capability
            // parameter carries no data and never reaches C
            // (`crosses_to_c`); everything else crosses at cancho's
            // own widths, mirroring `cancho-codegen`'s own
            // `Callee::Extern` arm.
            Callee::Extern(index) => {
                let ext = &self.program.externs[*index as usize];
                let mut param_kinds: Vec<LKind> = Vec::new();
                for param in ext.params.iter().filter(|t| crosses_to_c(t)) {
                    param_kinds.extend(leaves_of(param, self.program)?);
                }
                let flat: Vec<LValue> = evaluated
                    .into_iter()
                    .zip(&ext.params)
                    .filter(|(_, param)| crosses_to_c(param))
                    .flat_map(|(values, _)| values)
                    .collect();
                if flat.len() != param_kinds.len() {
                    return Err(format!(
                        "`{}` takes {} leaves but {} were given",
                        ext.symbol,
                        param_kinds.len(),
                        flat.len()
                    ));
                }
                let printed: Vec<String> = param_kinds
                    .iter()
                    .zip(&flat)
                    .map(|(kind, value)| format!("{} {}", kind.llvm(), operand(value)))
                    .collect();
                // `docs/reach.md` §3.4: `c_int` was declared `i32` in
                // `emit.rs`'s own declare loop, so the call here has to
                // say the same width -- `emit_call`'s shared "one leaf to
                // a register" path assumes the call's type matches the
                // *declared* signature, which for a narrow return it
                // would not. Sign-extend back to this backend's own
                // 64-bit `int` right after, the one place that reads the
                // real ABI width rather than trusting the rest of the
                // register.
                if ext.narrow_return && matches!(ext.ret, Type::Int) {
                    let raw = self.fresh();
                    self.out.push_str(&format!(
                        "  {raw} = call i32 @{}({})\n",
                        ext.symbol,
                        printed.join(", ")
                    ));
                    let extended = self.fresh();
                    self.out.push_str(&format!("  {extended} = sext i32 {raw} to i64\n"));
                    return Ok(vec![LValue::Reg(extended)]);
                }
                // `Type::Unit` (no `-> Type` in the declaration) is
                // `void`; it is otherwise unwritable, the same special
                // case `emit.rs`'s own declare loop makes.
                let ret_kinds = if matches!(ext.ret, Type::Unit) {
                    Vec::new()
                } else {
                    leaves_of(&ext.ret, self.program)?
                };
                Ok(self.emit_call(&ext.symbol, &printed, &ret_kinds))
            }
        }
    }

    /// One `call` instruction and its result unpacked -- `void` to
    /// nothing, one leaf to a register, more than one to the same
    /// `extractvalue` chain `checked_arith` already reads `{i64, i1}`
    /// out of LLVM's overflow intrinsics with. Shared between
    /// `Callee::Fn` and `Callee::Extern` above, which differ only in
    /// where the symbol and signature come from.
    fn emit_call(&mut self, symbol: &str, printed: &[String], ret_kinds: &[LKind]) -> Vec<LValue> {
        match ret_kinds {
            [] => {
                self.out.push_str(&format!("  call void @{symbol}({})\n", printed.join(", ")));
                Vec::new()
            }
            [kind] => {
                let result = self.fresh();
                self.out.push_str(&format!(
                    "  {result} = call {} @{symbol}({})\n",
                    kind.llvm(),
                    printed.join(", ")
                ));
                vec![LValue::Reg(result)]
            }
            kinds => {
                let ty = struct_ty(kinds);
                let agg = self.fresh();
                self.out
                    .push_str(&format!("  {agg} = call {ty} @{symbol}({})\n", printed.join(", ")));
                let mut unpacked = Vec::with_capacity(kinds.len());
                for (i, _) in kinds.iter().enumerate() {
                    let reg = self.fresh();
                    self.out.push_str(&format!("  {reg} = extractvalue {ty} {agg}, {i}\n"));
                    unpacked.push(LValue::Reg(reg));
                }
                unpacked
            }
        }
    }

    /// `docs/function-values.md` §4.2: a call through a value rather
    /// than a name. Everything [`Self::emit_call`] does, minus the one
    /// thing it cannot share: the callee here is an operand (`%tN`,
    /// `Type::CPtr`'s and `Type::Fn`'s shared `ptr` kind), not a global
    /// symbol, and with LLVM's opaque pointers a `ptr` value carries no
    /// signature of its own -- unlike a direct call to `@symbol`, whose
    /// declaration already states one, an indirect call has to spell
    /// the callee's parameter types explicitly, `call <ret> (<params>)
    /// <callee>(<args>)`, or LLVM has no way to know how many bytes of
    /// registers or stack the call touches.
    fn emit_call_indirect(
        &mut self,
        callee: &str,
        param_kinds: &[LKind],
        printed: &[String],
        ret_kinds: &[LKind],
    ) -> Vec<LValue> {
        let params: Vec<&str> = param_kinds.iter().map(|k| k.llvm()).collect();
        let sig = format!("({})", params.join(", "));
        match ret_kinds {
            [] => {
                self.out.push_str(&format!("  call void {sig} {callee}({})\n", printed.join(", ")));
                Vec::new()
            }
            [kind] => {
                let result = self.fresh();
                self.out.push_str(&format!(
                    "  {result} = call {} {sig} {callee}({})\n",
                    kind.llvm(),
                    printed.join(", ")
                ));
                vec![LValue::Reg(result)]
            }
            kinds => {
                let ty = struct_ty(kinds);
                let agg = self.fresh();
                self.out.push_str(&format!(
                    "  {agg} = call {ty} {sig} {callee}({})\n",
                    printed.join(", ")
                ));
                let mut unpacked = Vec::with_capacity(kinds.len());
                for (i, _) in kinds.iter().enumerate() {
                    let reg = self.fresh();
                    self.out.push_str(&format!("  {reg} = extractvalue {ty} {agg}, {i}\n"));
                    unpacked.push(LValue::Reg(reg));
                }
                unpacked
            }
        }
    }

    /// [`Expr::CallIndirect`]: `params`/`ret` are the callee's own type,
    /// carried on the node because there is no declaration to read a
    /// signature from at an indirect call site (mirrors `Callee::Fn`'s
    /// own arm in [`Self::call`], reading `params`/`ret` from the node
    /// instead of from `Program::funcs`).
    fn call_indirect(
        &mut self,
        target: &Expr,
        args: &[Expr],
        params: &[Type],
        ret: &Type,
    ) -> Result<Vec<LValue>, String> {
        let addr = self.scalar(target)?;
        let param_kinds: Vec<LKind> = params
            .iter()
            .map(|ty| leaves_of(ty, self.program))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        let evaluated: Vec<Vec<LValue>> =
            args.iter().map(|a| self.expr(a)).collect::<Result<_, _>>()?;
        let flat: Vec<LValue> = evaluated.into_iter().flatten().collect();
        if flat.len() != param_kinds.len() {
            return Err(format!(
                "an indirect call takes {} leaves but {} were given",
                param_kinds.len(),
                flat.len()
            ));
        }
        let printed: Vec<String> = param_kinds
            .iter()
            .zip(&flat)
            .map(|(kind, value)| format!("{} {}", kind.llvm(), operand(value)))
            .collect();
        let ret_kinds = leaves_of(ret, self.program)?;
        Ok(self.emit_call_indirect(&operand(&addr), &param_kinds, &printed, &ret_kinds))
    }
}
