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
            // the negation -- `lex-sys-codegen`'s own `Expr::Not` arm
            // (`body/expr.rs`), one instruction and never trapping.
            Expr::Not(inner) => {
                let v = self.scalar(inner)?;
                let flipped = self.fresh();
                self.out.push_str(&format!("  {flipped} = xor i8 {}, 1\n", operand(&v)));
                Ok(vec![LValue::Reg(flipped)])
            }
            // `-x`: on `int`, `0 - x`, checked -- `-int::MIN` has no
            // positive counterpart, the one place negation overflows,
            // the same reasoning `lex-sys-codegen`'s own `Expr::Neg`
            // uses. On `float`, `fneg` is total, flipping the sign bit
            // even on NaN and on zero, where it is what produces `-0.0`
            // (`docs/floating-point.md` §2) -- never a trap the way
            // `int`'s is.
            Expr::Neg(inner) => {
                let v = self.scalar(inner)?;
                if self.scalar_kind(inner)? == LKind::F64 {
                    let result = self.fresh();
                    self.out.push_str(&format!("  {result} = fneg double {}\n", operand(&v)));
                    return Ok(vec![LValue::Reg(result)]);
                }
                self.checked_arith("ssub", LValue::Const(0), v)
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
            // `docs/standard-input.md` §3: the mirror of `putchar`, no
            // argument to erase or narrow -- `getchar`'s own `io: &!i Io`
            // is already zero leaves. Sign-extended, not zero-extended:
            // `EOF` is `-1`, and zero-extending would hand the program
            // 4294967295, which is a byte-range check that silently
            // never fires.
            Callee::Builtin(Builtin::GetChar) => {
                let result = self.fresh();
                self.out.push_str(&format!("  {result} = call i32 @getchar()\n"));
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
                let result = self.fresh();
                self.out.push_str(&format!(
                    "  {result} = call i64 @fwrite(ptr {}, i64 1, i64 {}, ptr {stream})\n",
                    operand(start),
                    operand(len)
                ));
                Ok(vec![LValue::Reg(result)])
            }
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
                let length = self.fresh();
                self.out.push_str(&format!("  {length} = call i64 @strlen(ptr {text})\n"));
                Ok(vec![LValue::Reg(text), LValue::Reg(length)])
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
            // `select`-over-a-NaN-test `lex-sys-codegen`'s own `BitsOf`
            // already does.
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
            // the riddle `lex-sys-codegen`'s own `IsNan` is named for.
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
            // be library code, matching `lex-sys-codegen`'s own bare
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
