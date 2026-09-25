//! Memory: arenas, allocation (single-value and slice, arena and
//! heap), addressing, and the plain leaf load/store primitives
//! everything else here is built from.

use crate::*;

impl<'a> FuncEmitter<'a> {
    pub(crate) fn region_stmt(&mut self, arena: u32, body: &[Stmt]) -> Result<bool, String> {
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
    pub(crate) fn bump(&mut self, arena: u32, bytes: &LValue) -> Result<LValue, String> {
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
    pub(crate) fn fill_slice(
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
    pub(crate) fn slice_bytes(&mut self, count: &LValue, stride: i64) -> Result<LValue, String> {
        let negative = self.fresh();
        self.out.push_str(&format!("  {negative} = icmp slt i64 {}, 0\n", operand(count)));
        self.trap_if(&negative)?;

        let bytes = self.checked_arith("smul", count.clone(), LValue::Const(stride))?;
        Ok(bytes.into_iter().next().expect("`checked_arith` returns exactly one value"))
    }

    /// `alloc_slice[a](count, fill)` -- `count` copies of `fill`,
    /// contiguous (`lex-sys-codegen`'s own `alloc_slice`, `body/
    /// memory.rs`).
    pub(crate) fn alloc_slice(
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
    pub(crate) fn boxed_slice(
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

    /// How many bytes one whole value of this type occupies -- every leaf
    /// is 8 bytes here, the same stride every other slot in this backend
    /// uses, matching `lex-sys-codegen`'s own `leaf_count(ty) *
    /// RETURN_SLOT_STRIDE` (`body/memory.rs`). Unlike `stride_of`, which
    /// special-cases a `[T]` element's own `byte` to 1, a whole value is
    /// never a bare `byte` here -- `alloc`/`box`/`unbox` all take a
    /// struct or scalar `Type`, not an element type.
    pub(crate) fn value_bytes(&self, ty: &Type) -> Result<i64, String> {
        Ok(leaves_of(ty, self.program)?.len() as i64 * 8)
    }

    /// `alloc[a](value)` -- bump-allocate and write the value there, the
    /// same `bump` helper `alloc_slice` already opened (§7.5) --
    /// `lex-sys-codegen`'s own `alloc` (`body/memory.rs`), one value
    /// rather than a fill loop over many.
    pub(crate) fn alloc(&mut self, arena: u32, ty: &Type, value: &Expr) -> Result<LValue, String> {
        let values = self.expr(value)?;
        let kinds = leaves_of(ty, self.program)?;
        let bytes = LValue::Const(self.value_bytes(ty)?);
        let at = self.bump(arena, &bytes)?;
        self.store_leaves(&operand(&at), &kinds, &values);
        Ok(at)
    }

    /// `box(h, value)` -- one `malloc`, trapping on exhaustion exactly as
    /// an arena's own bump does, and the value written into it
    /// (`lex-sys-codegen`'s own `boxed`, `body/memory.rs`) -- the same
    /// sizing and null check `boxed_slice` already makes, minus its fill
    /// loop: one value here, not `count` copies of one.
    pub(crate) fn boxed(&mut self, ty: &Type, value: &Expr) -> Result<LValue, String> {
        let values = self.expr(value)?;
        let kinds = leaves_of(ty, self.program)?;
        let bytes = self.value_bytes(ty)?;
        let at = self.fresh();
        self.out.push_str(&format!("  {at} = call ptr @malloc(i64 {bytes})\n"));
        let is_null = self.fresh();
        self.out.push_str(&format!("  {is_null} = icmp eq ptr {at}, null\n"));
        self.trap_if(&is_null)?;
        self.store_leaves(&at, &kinds, &values);
        Ok(LValue::Reg(at))
    }

    /// `unbox(h, b)` -- read the value back, then one `free`. The load
    /// has to happen before the free, the only ordering constraint here,
    /// matching `lex-sys-codegen`'s own `unboxed` (`body/memory.rs`)
    /// exactly, including why it is one function rather than two
    /// composable ones.
    pub(crate) fn unboxed(&mut self, ty: &Type, value: &Expr) -> Result<Vec<LValue>, String> {
        let leaves = self.expr(value)?;
        let [at] = leaves.as_slice() else {
            return Err("`unbox`'s argument is not a box (expected 1 leaf: a pointer)".to_owned());
        };
        let kinds = leaves_of(ty, self.program)?;
        let values = self.load_leaves(&operand(at), &kinds);
        self.out.push_str(&format!("  call void @free(ptr {})\n", operand(at)));
        Ok(values)
    }

    /// `if`/`else`. Returns whether *both* arms terminate -- the checker's
    /// own `terminates()` rule, and the reason a terminating `if` is
    /// always a block's last statement: nothing here needs to merge a
    /// live value between the two arms, because every local this backend
    /// has is memory (`docs/llvm-backend.md` §5's own note on why this
    /// crate never builds a `phi`), so the block after the `if` simply
    /// reads whatever the taken arm last wrote.
    pub(crate) fn store_leaves(&mut self, buffer: &str, kinds: &[LKind], values: &[LValue]) {
        for (leaf, (kind, value)) in kinds.iter().zip(values).enumerate() {
            let addr = self.fresh();
            self.out.push_str(&format!(
                "  {addr} = getelementptr i8, ptr {buffer}, i64 {}\n",
                leaf as i64 * 8
            ));
            self.out.push_str(&format!("  store {} {}, ptr {addr}\n", kind.llvm(), operand(value)));
        }
    }

    pub(crate) fn load_leaves(&mut self, buffer: &str, kinds: &[LKind]) -> Vec<LValue> {
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

    /// A struct field's leaf offset among its declaration, and the
    /// field's own kinds -- shared by every way of reaching one
    /// (`docs/llvm-backend.md` §7.11): `Expr::Field` picks the field's
    /// leaves out of an owned value already in registers; `Expr::
    /// FieldRef`/`Expr::FieldAddr`/`Place::Field` all reach the same
    /// field through a reference instead, where the offset is bytes
    /// into a referent rather than a position in a `Vec`.
    pub(crate) fn field_offset(
        &self,
        def: DefId,
        args: &[Type],
        index: u32,
    ) -> Result<(i64, Vec<LKind>), String> {
        let lex_sys_ir::TypeInfo::Struct { fields, .. } = self.program.type_info(def) else {
            return Err("a field access on an enum is not part of this slice".to_owned());
        };
        let mut start = 0i64;
        for (_, ty) in &fields[..index as usize] {
            start += leaves_of(&ty.substitute(args, &[]), self.program)?.len() as i64;
        }
        let kinds = leaves_of(&fields[index as usize].1.substitute(args, &[]), self.program)?;
        Ok((start * 8, kinds))
    }

    /// The distance between elements of a `[T]`, matching
    /// `lex-sys-codegen`'s own `abi::stride_of`: a byte is the one size
    /// in the language not a multiple of 8 (`docs/strings.md` §3), so
    /// that a string is something C could read.
    pub(crate) fn stride_of(&self, element: &Type) -> Result<i64, String> {
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
    pub(crate) fn element_address(
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

    /// `s[a..b]` -- a half-open run (`docs/slicing.md`, §7.9). The same
    /// two comparisons an index does, against the same length already in
    /// the slice's second leaf, and then the arithmetic that makes a
    /// slice: a pointer and a length. Nothing is copied and nothing is
    /// allocated -- `lex-sys-codegen`'s own `subslice`, `body/memory.rs`.
    /// `a > b` traps rather than yielding empty (`docs/defined-
    /// behaviour.md` §2.1): an inverted range is a bug in the program
    /// that wrote it.
    pub(crate) fn subslice(
        &mut self,
        base: &Expr,
        start: &Expr,
        end: &Expr,
        element: &Type,
    ) -> Result<Vec<LValue>, String> {
        let values = self.expr(base)?;
        if values.len() != 2 {
            return Err("slicing needs a slice (expected 2 leaves: pointer and length)".to_owned());
        }
        let (ptr, len) = (values[0].clone(), values[1].clone());
        let start = self.scalar(start)?;
        let end = self.scalar(end)?;

        let past = self.fresh();
        self.out.push_str(&format!(
            "  {past} = icmp ugt i64 {}, {}\n",
            operand(&end),
            operand(&len)
        ));
        self.trap_if(&past)?;
        let inverted = self.fresh();
        self.out.push_str(&format!(
            "  {inverted} = icmp ugt i64 {}, {}\n",
            operand(&start),
            operand(&end)
        ));
        self.trap_if(&inverted)?;

        let stride = self.stride_of(element)?;
        let offset = self.fresh();
        self.out.push_str(&format!("  {offset} = mul i64 {}, {stride}\n", operand(&start)));
        let addr = self.fresh();
        self.out.push_str(&format!(
            "  {addr} = getelementptr i8, ptr {}, i64 {offset}\n",
            operand(&ptr)
        ));
        let length = self.fresh();
        self.out.push_str(&format!(
            "  {length} = sub i64 {}, {}\n",
            operand(&end),
            operand(&start)
        ));
        Ok(vec![LValue::Reg(addr), LValue::Reg(length)])
    }
}
