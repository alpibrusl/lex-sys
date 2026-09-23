//! Where values live: libc, arenas, paths and files, the command line,
//! byte literals and statics, slices, and the heap.

use crate::*;

impl<'a, 'f> BodyEmitter<'a, 'f> {
    /// `borrow x as &r in { .. }` — give `x` a home in memory and point at it.
    ///
    /// A reference has to be an address, and until now nothing did: a slot
    /// lives in SSA variables, which have none. So the referent's leaves are
    /// spilled into a buffer for the duration of the block and the reference
    /// holds that buffer's address.
    ///
    /// Nothing is written back when the block closes, and nothing needs to
    /// be: the checker froze the referent for the whole region, so the
    /// variables and the buffer cannot have drifted apart. A unique borrow
    /// will need the copy back, and the buffer is where it will come from.
    /// Declare a libc symbol the backend reaches for itself, on first use.
    ///
    /// `Module::declare_function` is idempotent for one name and signature,
    /// so after the first call this is a lookup. Declaring on use rather
    /// than up front is what keeps a program with no `region` block free of
    /// an import it never makes — and what stops the symbol assertions in
    /// the tests below from passing vacuously.
    pub(crate) fn libc_fn(
        &mut self,
        name: &str,
        params: &[types::Type],
        returns: &[types::Type],
    ) -> FuncId {
        let mut sig = self.module.make_signature();
        sig.call_conv = self.module.isa().default_call_conv();
        for param in params {
            sig.params.push(AbiParam::new(*param));
        }
        for ret in returns {
            sig.returns.push(AbiParam::new(*ret));
        }
        self.module
            .declare_function(name, Linkage::Import, &sig)
            .expect("libc's own symbols are declared consistently")
    }

    /// `free(pointer)` — release one arena's chunk.
    pub(crate) fn free(&mut self, held: Value) {
        let pointer = self.pointer;
        let id = self.libc_fn("free", &[pointer], &[]);
        let f = self.module.declare_func_in_func(id, self.builder.func);
        self.builder.ins().call(f, &[held]);
    }

    /// `region a { .. }` — open an arena, run the body, release it (§6).
    ///
    /// One `malloc` in, one `free` out. Between them the arena is two
    /// pointers: where the next allocation goes, and where the chunk ends.
    /// The end is not stored, because it is the base plus a constant.
    ///
    /// Release is a single `free` whatever was allocated — no traversal and
    /// no per-object bookkeeping, which is the property §6 is trading
    /// expressiveness for. Nothing runs at teardown because nothing *can*:
    /// §6.1 keeps `res` values out, so there is no obligation left inside to
    /// discharge.
    pub(crate) fn region_stmt(&mut self, arena: u32, body: &[Stmt]) -> bool {
        let pointer = self.pointer;
        let size = self.builder.ins().iconst(pointer, ARENA_CHUNK);
        let id = self.libc_fn("malloc", &[pointer], &[pointer]);
        let f = self.module.declare_func_in_func(id, self.builder.func);
        let call = self.builder.ins().call(f, &[size]);
        let base = self.builder.inst_results(call)[0];
        // Out of memory is a trap, not a null pointer wandering into a
        // store. The language has no undefined behaviour to fall back on.
        self.builder.ins().trapz(base, TrapCode::HEAP_OUT_OF_BOUNDS);

        let base_var = self.temporary(pointer);
        let bump_var = self.temporary(pointer);
        self.builder.def_var(base_var, base);
        self.builder.def_var(bump_var, base);
        // Grow to fit rather than push: a sibling `region` carries a
        // higher number than one already closed, so the slot may be past
        // the end and the ones before it may be empty.
        if self.arenas.len() <= arena as usize {
            self.arenas.resize(arena as usize + 1, None);
        }
        self.arenas[arena as usize] = Some((base_var, bump_var));

        let returned = self.stmts(body);

        // Skipped when the body returned: `emit_return` already released
        // this arena on the way out, and there is no block left to put a
        // second call in.
        if !returned {
            let held = self.builder.use_var(base_var);
            self.free(held);
        }
        self.arenas[arena as usize] = None;
        returned
    }

    /// Take `bytes` from an arena, trapping if the chunk cannot spare them.
    ///
    /// Shared by `alloc` and `alloc_slice`, which differ only in how many
    /// bytes they ask for and what they write there.
    pub(crate) fn bump(&mut self, arena: u32, bytes: Value) -> Value {
        let (base_var, bump_var) =
            self.arenas[arena as usize].expect("`alloc` names an arena open here");
        let at = self.builder.use_var(bump_var);
        let next = self.builder.ins().iadd(at, bytes);

        let base = self.builder.use_var(base_var);
        let end = self.builder.ins().iadd_imm(base, ARENA_CHUNK);
        // Two ways to be past the end, and a slice can hit either: the sum
        // overshoots the chunk, or the size computation itself wrapped and
        // the sum came out *below* where it started. Both are refused here
        // rather than trusted to a length nobody checked.
        let over = self.builder.ins().icmp(IntCC::UnsignedGreaterThan, next, end);
        let wrapped = self.builder.ins().icmp(IntCC::UnsignedLessThan, next, at);
        let bad = self.builder.ins().bor(over, wrapped);
        self.builder.ins().trapnz(bad, TrapCode::HEAP_OUT_OF_BOUNDS);

        self.builder.def_var(bump_var, next);
        at
    }

    /// The longest path a file operation will build, including the NUL.
    ///
    /// A fixed buffer because the path is copied onto the stack to be
    /// NUL-terminated — C wants a terminator and a slice does not carry one
    /// (`docs/strings.md` §6). A longer path traps rather than being cut
    /// short, because a silently truncated path names a different file.
    pub(crate) fn checked_path(&mut self, prefix: &str, path: &[Value]) -> Value {
        const PATH_MAX: i64 = 4096;
        let pointer = self.pointer;
        let (source, length) = (path[0], path[1]);

        // Room for the bytes and the NUL.
        let too_long =
            self.builder.ins().icmp_imm(IntCC::UnsignedGreaterThanOrEqual, length, PATH_MAX);
        self.builder.ins().trapnz(too_long, TrapCode::HEAP_OUT_OF_BOUNDS);

        // A path shorter than the prefix cannot start with it.
        let short =
            self.builder.ins().icmp_imm(IntCC::UnsignedLessThan, length, prefix.len() as i64);
        self.builder.ins().trapnz(short, TrapCode::HEAP_OUT_OF_BOUNDS);

        let slot = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            PATH_MAX as u32,
            0,
        ));
        let buffer = self.builder.ins().stack_addr(pointer, slot, 0);

        // The prefix the capability was narrowed to, as data to compare
        // against. It is known at compile time; the path is not, which is
        // the whole reason this check is here rather than in the checker
        // (`docs/filesystem.md` §4).
        let expected = self.bytes(prefix)[0];

        // One pass: copy the byte, check it against the prefix while we are
        // still inside it, and refuse `..` anywhere at all.
        let header = self.builder.create_block();
        let body = self.builder.create_block();
        let done = self.builder.create_block();
        let cursor = self.temporary(types::I64);
        let previous = self.temporary(types::I8);
        let zero = self.builder.ins().iconst(types::I64, 0);
        let none = self.builder.ins().iconst(types::I8, 0);
        self.builder.def_var(cursor, zero);
        self.builder.def_var(previous, none);
        self.builder.ins().jump(header, &[]);

        self.builder.switch_to_block(header);
        let i = self.builder.use_var(cursor);
        let more = self.builder.ins().icmp(IntCC::UnsignedLessThan, i, length);
        self.builder.ins().brif(more, body, &[], done, &[]);

        self.builder.switch_to_block(body);
        self.builder.seal_block(body);
        let i = self.builder.use_var(cursor);
        let at = self.builder.ins().iadd(source, i);
        let byte = self.builder.ins().load(types::I8, MemFlags::trusted(), at, 0);
        let into = self.builder.ins().iadd(buffer, i);
        self.builder.ins().store(MemFlags::trusted(), byte, into, 0);

        // `..` is refused rather than normalised (§4.1): normalising is a
        // security function with its own design, and a prefix check that
        // quietly admits `/tmp/../etc` would be the dishonest third option.
        let dot = self.builder.ins().icmp_imm(IntCC::Equal, byte, i64::from(b'.'));
        let prior = self.builder.use_var(previous);
        let prior_dot = self.builder.ins().icmp_imm(IntCC::Equal, prior, i64::from(b'.'));
        let traversal = self.builder.ins().band(dot, prior_dot);
        self.builder.ins().trapnz(traversal, TrapCode::HEAP_OUT_OF_BOUNDS);
        self.builder.def_var(previous, byte);

        // Inside the prefix, the bytes have to match. A path outside what
        // the capability granted is a broken promise, so it traps rather
        // than returning -1.
        let inside = self.builder.ins().icmp_imm(IntCC::UnsignedLessThan, i, prefix.len() as i64);
        let want_at = self.builder.ins().iadd(expected, i);
        let want = self.builder.ins().load(types::I8, MemFlags::trusted(), want_at, 0);
        let differs = self.builder.ins().icmp(IntCC::NotEqual, byte, want);
        let escaped = self.builder.ins().band(inside, differs);
        self.builder.ins().trapnz(escaped, TrapCode::HEAP_OUT_OF_BOUNDS);

        let next = self.builder.ins().iadd_imm(i, 1);
        self.builder.def_var(cursor, next);
        self.builder.ins().jump(header, &[]);
        self.builder.seal_block(header);

        self.builder.switch_to_block(done);
        self.builder.seal_block(done);
        let end = self.builder.ins().iadd(buffer, length);
        self.builder.ins().store(MemFlags::trusted(), none, end, 0);

        // The bytes match, which is not yet the same as being inside the
        // directory: `/tmp` does not contain `/tmpevil` (§1). The path
        // either *is* the granted name or continues from it at a `/`, and
        // the NUL is already stored, so the byte one past a path of exactly
        // the prefix's length reads as zero rather than as rubbish.
        if !prefix.is_empty() && !prefix.ends_with('/') {
            let at = self.builder.ins().iadd_imm(buffer, prefix.len() as i64);
            let byte = self.builder.ins().load(types::I8, MemFlags::trusted(), at, 0);
            let separator = self.builder.ins().icmp_imm(IntCC::Equal, byte, i64::from(b'/'));
            let ended = self.builder.ins().icmp_imm(IntCC::Equal, byte, 0);
            let boundary = self.builder.ins().bor(separator, ended);
            self.builder.ins().trapz(boundary, TrapCode::HEAP_OUT_OF_BOUNDS);
        }
        buffer
    }

    /// `fs_read` and `fs_write` (`docs/filesystem.md` §3).
    ///
    /// The backend reaches libc itself rather than the program declaring an
    /// `extern fn`, because an `extern` would be gated by `Ffi("libc")` and
    /// then the FFI capability would open any path, with `Fs` contributing
    /// nothing (§2).
    pub(crate) fn file_op(&mut self, write: bool, prefix: &str, args: &[Expr]) -> Vec<Value> {
        let pointer = self.pointer;
        // The capability is zero-sized and stops here; the two slices do not.
        let path = self.expr(&args[1]);
        let bytes = self.expr(&args[2]);
        let path = self.checked_path(prefix, &path);

        // **Neither call here is `open(path, flags, mode)`**, and that is
        // deliberate. `open` is variadic — `int open(const char *, int, ...)`
        // — and on Apple ARM64 a variadic argument is passed on the *stack*
        // while a fixed one is passed in a register. Declaring it with three
        // fixed arguments puts `mode` in a register the callee never reads,
        // so the file is created with whatever was on the stack: the write
        // succeeds, the permissions are junk, and reading the file back
        // fails. Linux x86-64 hides this, because there varargs and fixed
        // arguments share the same registers.
        //
        // So: `creat(path, mode)` for writing, which is exactly
        // `open(path, O_WRONLY|O_CREAT|O_TRUNC, mode)` and is *not*
        // variadic — it also deletes the platform-dependent flag constants,
        // which were the other thing here a portable language should not be
        // guessing at. And `open(path, O_RDONLY)` for reading, declared with
        // two arguments: that is the non-variadic prefix, so no argument of
        // ours lands anywhere the callee is not looking, and `O_RDONLY` is
        // zero on both platforms.
        let fd = if write {
            let creat = self.libc_fn("creat", &[pointer, types::I32], &[types::I32]);
            let creat = self.module.declare_func_in_func(creat, self.builder.func);
            let mode = self.builder.ins().iconst(types::I32, 0o644);
            let call = self.builder.ins().call(creat, &[path, mode]);
            self.builder.inst_results(call)[0]
        } else {
            let open = self.libc_fn("open", &[pointer, types::I32], &[types::I32]);
            let open = self.module.declare_func_in_func(open, self.builder.func);
            let read_only = self.builder.ins().iconst(types::I32, 0);
            let call = self.builder.ins().call(open, &[path, read_only]);
            self.builder.inst_results(call)[0]
        };

        // A missing file is an ordinary outcome, not a broken promise, so
        // it is `-1` rather than a trap (§3).
        let failed = self.builder.create_block();
        let opened = self.builder.create_block();
        let merge = self.builder.create_block();
        self.builder.append_block_param(merge, types::I64);
        let bad = self.builder.ins().icmp_imm(IntCC::SignedLessThan, fd, 0);
        self.builder.ins().brif(bad, failed, &[], opened, &[]);

        self.builder.switch_to_block(failed);
        self.builder.seal_block(failed);
        let minus_one = self.builder.ins().iconst(types::I64, -1);
        self.builder.ins().jump(merge, &[minus_one.into()]);

        self.builder.switch_to_block(opened);
        self.builder.seal_block(opened);
        let name = if write { "write" } else { "read" };
        let transfer = self.libc_fn(name, &[types::I32, pointer, types::I64], &[types::I64]);
        let transfer = self.module.declare_func_in_func(transfer, self.builder.func);
        let call = self.builder.ins().call(transfer, &[fd, bytes[0], bytes[1]]);
        let moved = self.builder.inst_results(call)[0];

        let close = self.libc_fn("close", &[types::I32], &[types::I32]);
        let close = self.module.declare_func_in_func(close, self.builder.func);
        self.builder.ins().call(close, &[fd]);
        self.builder.ins().jump(merge, &[moved.into()]);

        self.builder.switch_to_block(merge);
        self.builder.seal_block(merge);
        vec![self.builder.block_params(merge)[0]]
    }

    /// `open_read(fs, path)` — `docs/file-handles.md` §2.1.
    ///
    /// The first half of [`Self::file_op`] and then it stops: the same
    /// prefix check, the same non-variadic `open(path, O_RDONLY)`, and the
    /// descriptor *kept* rather than spent on one transfer and closed. What
    /// comes back is an `Opened`, which is a tag and one payload leaf.
    pub(crate) fn open_file(&mut self, prefix: &str, args: &[Expr]) -> Vec<Value> {
        let pointer = self.pointer;
        // The capability is zero-sized and stops here; the path does not.
        let path = self.expr(&args[1]);
        let path = self.checked_path(prefix, &path);

        let open = self.libc_fn("open", &[pointer, types::I32], &[types::I32]);
        let open = self.module.declare_func_in_func(open, self.builder.func);
        let read_only = self.builder.ins().iconst(types::I32, 0);
        let call = self.builder.ins().call(open, &[path, read_only]);
        let fd = self.builder.inst_results(call)[0];
        let fd = self.builder.ins().sextend(types::I64, fd);

        // Tag 0 is `Ok(File)` and tag 1 is `Failed(int)`, which is
        // declaration order in the prelude. Each variant has its own leaf
        // -- a payload slot is not shared between arms -- so this is three
        // values: the tag, the descriptor, and the reason.
        let failed = self.builder.ins().icmp_imm(IntCC::SignedLessThan, fd, 0);
        let one = self.builder.ins().iconst(types::I64, 1);
        let zero = self.builder.ins().iconst(types::I64, 0);
        let tag = self.builder.ins().select(failed, one, zero);
        let reason = self.errno();
        vec![tag, fd, reason]
    }

    /// `errno`, which is a *function* in every modern libc because it has
    /// to be per-thread: `__errno_location()` on glibc and `__error()` on
    /// macOS, both answering a pointer to it.
    ///
    /// This is `docs/file-handles.md` §6's last row, and the half of §1's
    /// argument for handles that survived the other two evaporating:
    /// `fs_read` answers `-1` with no reason attached, so `examples/sort/`
    /// could name the path it could not read and not why. `Failed(int)`
    /// carries the number, and a program can say *No such file* where it
    /// used to say only *failed*.
    pub(crate) fn errno(&mut self) -> Value {
        let pointer = self.pointer;
        let symbol = match self.module.isa().triple().operating_system {
            target_lexicon::OperatingSystem::Darwin(_) => "__error",
            _ => "__errno_location",
        };
        let id = self.libc_fn(symbol, &[], &[pointer]);
        let at = self.module.declare_func_in_func(id, self.builder.func);
        let call = self.builder.ins().call(at, &[]);
        let address = self.builder.inst_results(call)[0];
        let value = self.builder.ins().load(types::I32, MemFlags::trusted(), address, 0);
        self.builder.ins().sextend(types::I64, value)
    }

    /// `read(file, into)` — `docs/file-handles.md` §3.
    ///
    /// One `read(2)`, and its three outcomes sorted into the three
    /// constructors. The descriptor comes in as the handle's one leaf, and
    /// the buffer as the pointer and length a slice always is.
    pub(crate) fn read_file(&mut self, args: &[Value]) -> Vec<Value> {
        let pointer = self.pointer;
        // The handle arrives as a reference, so the descriptor is a load;
        // the buffer is the pointer and length a slice always is.
        let fd = self.builder.ins().load(types::I64, MemFlags::trusted(), args[0], 0);
        let fd = self.builder.ins().ireduce(types::I32, fd);

        let read = self.libc_fn("read", &[types::I32, pointer, types::I64], &[types::I64]);
        let read = self.module.declare_func_in_func(read, self.builder.func);
        let call = self.builder.ins().call(read, &[fd, args[1], args[2]]);
        let moved = self.builder.inst_results(call)[0];

        // Tags in declaration order: `Got(int)` 0, `End` 1, `Failed(int)` 2.
        // A negative count is a failure, a zero is the end, and anything
        // else is bytes -- which is the whole of `read(2)`'s contract and
        // the last place this program has to know it.
        let negative = self.builder.ins().icmp_imm(IntCC::SignedLessThan, moved, 0);
        let empty = self.builder.ins().icmp_imm(IntCC::Equal, moved, 0);
        let two = self.builder.ins().iconst(types::I64, 2);
        let one = self.builder.ins().iconst(types::I64, 1);
        let zero = self.builder.ins().iconst(types::I64, 0);
        let not_failed = self.builder.ins().select(empty, one, zero);
        let tag = self.builder.ins().select(negative, two, not_failed);
        // Three leaves: the tag, `Got`'s count, and `Failed`'s reason.
        // `End` carries nothing and so occupies none. Reading `errno` here
        // costs a call on the path that succeeded too, which is the price
        // of answering *why* rather than *whether* (§6).
        let reason = self.errno();
        vec![tag, moved, reason]
    }

    /// The address of one of `main`'s two globals
    /// (`docs/arguments.md` §3).
    pub(crate) fn global(&mut self, name: &str) -> Value {
        let pointer = self.pointer;
        let id = self
            .module
            .declare_data(name, Linkage::Local, true, false)
            .expect("the entry point declared this global consistently");
        let global = self.module.declare_data_in_func(id, self.builder.func);
        self.builder.ins().global_value(pointer, global)
    }

    /// `argc`, as the runtime handed it to `main`.
    pub(crate) fn argc(&mut self) -> Value {
        let at = self.global(ARGC_GLOBAL);
        self.builder.ins().load(types::I64, MemFlags::trusted(), at, 0)
    }

    /// A string literal: its bytes into read-only data, and the two leaves
    /// a slice is made of pointing at them (`docs/strings.md` §4).
    ///
    /// The data is `Local` and not writable, which is what makes the
    /// *shared* slice honest — there is no unique reference to it anywhere,
    /// and the section it lands in would refuse a write anyway.
    pub(crate) fn bytes(&mut self, text: &str) -> Vec<Value> {
        let pointer = self.pointer;
        let name = format!("{PREFIX}str_{}_{}", self.func.name, self.literals);
        self.literals += 1;

        let mut description = DataDescription::new();
        // An empty literal still needs an address, because a slice is a
        // pointer and a length and the pointer has to be *some*thing. One
        // byte nobody reads is the cheapest honest answer: the length is
        // zero, and every index is checked against it.
        let contents: Vec<u8> = if text.is_empty() { vec![0] } else { text.as_bytes().to_vec() };
        description.define(contents.into_boxed_slice());

        let id = self
            .module
            .declare_data(&name, Linkage::Local, false, false)
            .expect("a fresh name for each literal");
        self.module.define_data(id, &description).expect("each literal is defined once");

        let value = self.module.declare_data_in_func(id, self.builder.func);
        let start = self.builder.ins().global_value(pointer, value);
        let len = self.builder.ins().iconst(types::I64, text.len() as i64);
        vec![start, len]
    }

    /// A `static`'s data: a pointer into it and its length
    /// (`docs/compile-time-data.md` §2).
    ///
    /// Declared once per program and referenced by every reader, unlike a
    /// string literal which is defined per occurrence — a table is bigger
    /// than a greeting, and two copies of a 512 KB table is a size
    /// regression nobody asked for.
    pub(crate) fn static_data(&mut self, index: u32) -> Vec<Value> {
        let pointer = self.pointer;
        let data = &self.program.statics[index as usize];
        let name = format!("{PREFIX}static_{}", data.name);
        let len = data.values.len() as i64;

        let id = self
            .module
            .declare_data(&name, Linkage::Local, false, false)
            .expect("a static is declared once per program");
        let value = self.module.declare_data_in_func(id, self.builder.func);
        let start = self.builder.ins().global_value(pointer, value);
        let len = self.builder.ins().iconst(types::I64, len);
        vec![start, len]
    }

    /// `alloc_slice[a](count, fill)` — `count` copies of `fill`, contiguous.
    ///
    /// Returns the two leaves a slice is made of: where it starts and how
    /// many elements it has.
    pub(crate) fn alloc_slice(
        &mut self,
        arena: u32,
        element: &Type,
        count: &Expr,
        fill: &Expr,
    ) -> Vec<Value> {
        let count = self.scalar(count);
        let values = self.expr(fill);
        let stride = self.stride(element);
        let bytes = self.slice_bytes(count, stride);
        let start = self.bump(arena, bytes);
        self.fill_slice(start, count, stride, &values);
        vec![start, count]
    }

    /// `box_slice(h, count, fill)` — a run of values on the heap
    /// (`docs/boxed-slices.md` §3).
    ///
    /// The same sizing and the same fill as an arena slice; only where the
    /// memory comes from differs. What comes back is two leaves, a pointer
    /// *and* a length, because nothing else knows how many elements there
    /// are (§2).
    pub(crate) fn boxed_slice(&mut self, element: &Type, count: &Expr, fill: &Expr) -> Vec<Value> {
        let count = self.scalar(count);
        let values = self.expr(fill);
        let stride = self.stride(element);
        let bytes = self.slice_bytes(count, stride);

        let pointer = self.pointer;
        let id = self.libc_fn("malloc", &[pointer], &[pointer]);
        let f = self.module.declare_func_in_func(id, self.builder.func);
        let call = self.builder.ins().call(f, &[bytes]);
        let start = self.builder.inst_results(call)[0];
        // Out of memory traps, exactly as `box` and an exhausted arena do.
        self.builder.ins().trapz(start, TrapCode::HEAP_OUT_OF_BOUNDS);

        self.fill_slice(start, count, stride, &values);
        vec![start, count]
    }

    /// How many bytes `count` elements take, checked.
    ///
    /// A negative length is not a small allocation, it is a mistake, and
    /// reading `s[0]` of one would be reading memory nobody reserved.
    /// `count * stride` is checked for the reason every other
    /// multiplication is: a length that overflows the byte count would ask
    /// for less than is about to be written.
    pub(crate) fn slice_bytes(&mut self, count: Value, stride: i64) -> Value {
        let negative = self.builder.ins().icmp_imm(IntCC::SignedLessThan, count, 0);
        self.builder.ins().trapnz(negative, TrapCode::HEAP_OUT_OF_BOUNDS);
        let width = self.builder.ins().iconst(types::I64, stride);
        let (bytes, overflowed) = self.builder.ins().smul_overflow(count, width);
        self.builder.ins().trapnz(overflowed, TrapCode::INTEGER_OVERFLOW);
        bytes
    }

    /// Write `values` into every element of a freshly reserved run.
    ///
    /// A loop rather than an unrolled run, because the length is a runtime
    /// value -- which is what makes it a slice.
    pub(crate) fn fill_slice(&mut self, start: Value, count: Value, stride: i64, values: &[Value]) {
        let header = self.builder.create_block();
        let body = self.builder.create_block();
        let done = self.builder.create_block();
        let cursor = self.temporary(types::I64);
        let zero = self.builder.ins().iconst(types::I64, 0);
        self.builder.def_var(cursor, zero);
        self.builder.ins().jump(header, &[]);

        self.builder.switch_to_block(header);
        let i = self.builder.use_var(cursor);
        let more = self.builder.ins().icmp(IntCC::SignedLessThan, i, count);
        self.builder.ins().brif(more, body, &[], done, &[]);

        self.builder.switch_to_block(body);
        self.builder.seal_block(body);
        let i = self.builder.use_var(cursor);
        let offset = self.builder.ins().imul_imm(i, stride);
        let address = self.builder.ins().iadd(start, offset);
        self.store_leaves(address, values);
        let next = self.builder.ins().iadd_imm(i, 1);
        self.builder.def_var(cursor, next);
        self.builder.ins().jump(header, &[]);
        self.builder.seal_block(header);

        self.builder.switch_to_block(done);
        self.builder.seal_block(done);
    }

    /// How many bytes one element of a slice takes.
    ///
    /// Everything is leaf-stride apart except `byte`, which is packed one
    /// per byte (`docs/strings.md` §3): a string at 8 bytes per character
    /// could not be handed to C, and would not be a string so much as a
    /// rumour of one. This is the only size in the language that is not a
    /// multiple of 8, and it is confined to `byte` on purpose.
    pub(crate) fn stride(&self, element: &Type) -> i64 {
        match element {
            Type::Byte => 1,
            other => {
                i64::from(leaf_count(other, self.program, self.pointer))
                    * i64::from(RETURN_SLOT_STRIDE)
            }
        }
    }

    /// Where element `index` of a slice lives, with the bounds check in
    /// front of it (`docs/defined-behaviour.md` §1).
    ///
    /// One unsigned comparison covers both ends: a negative index read as
    /// unsigned is enormous, so `i >= len` catches it too.
    pub(crate) fn element_address(&mut self, base: &Expr, index: &Expr, element: &Type) -> Value {
        let slice = self.expr(base);
        let (start, len) = (slice[0], slice[1]);
        let index = self.scalar(index);

        let out_of_range = self.builder.ins().icmp(IntCC::UnsignedGreaterThanOrEqual, index, len);
        self.builder.ins().trapnz(out_of_range, TrapCode::HEAP_OUT_OF_BOUNDS);

        let stride = self.stride(element);
        let offset = self.builder.ins().imul_imm(index, stride);
        self.builder.ins().iadd(start, offset)
    }

    /// `s[a..b]` — a half-open run (`docs/slicing.md`).
    ///
    /// The same two comparisons an index does, against the same length that
    /// is already in the slice's second register, and then the arithmetic
    /// that makes a slice: a pointer and a length. Nothing is copied and
    /// nothing is allocated.
    ///
    /// `a > b` traps rather than yielding empty (§2): an inverted range is a
    /// bug in the program that wrote it, and a defined-but-wrong answer is
    /// what `defined-behaviour.md` §2.1 refuses.
    pub(crate) fn subslice(
        &mut self,
        base: &Expr,
        start: &Expr,
        end: &Expr,
        element: &Type,
    ) -> Vec<Value> {
        let slice = self.expr(base);
        let (at, len) = (slice[0], slice[1]);
        let start = self.scalar(start);
        let end = self.scalar(end);

        // `end > len` or `start > end` -- and an unsigned comparison catches
        // a negative bound as a very large one, exactly as indexing does.
        let past = self.builder.ins().icmp(IntCC::UnsignedGreaterThan, end, len);
        self.builder.ins().trapnz(past, TrapCode::HEAP_OUT_OF_BOUNDS);
        let inverted = self.builder.ins().icmp(IntCC::UnsignedGreaterThan, start, end);
        self.builder.ins().trapnz(inverted, TrapCode::HEAP_OUT_OF_BOUNDS);

        let stride = self.stride(element);
        let offset = self.builder.ins().imul_imm(start, stride);
        let address = self.builder.ins().iadd(at, offset);
        let length = self.builder.ins().isub(end, start);
        vec![address, length]
    }

    /// `alloc[a](value)` — bump-allocate and write the value there (§6).
    pub(crate) fn alloc(&mut self, arena: u32, ty: &Type, value: &Expr) -> Value {
        let values = self.expr(value);
        let bytes =
            i64::from(leaf_count(ty, self.program, self.pointer)) * i64::from(RETURN_SLOT_STRIDE);
        let size = self.builder.ins().iconst(self.pointer, bytes);
        let at = self.bump(arena, size);
        self.store_leaves(at, &values);
        at
    }

    /// The bytes one value of this type occupies on the heap.
    ///
    /// The same leaf-slot layout an arena allocation uses (§6), for the same
    /// reason: the backend stores and loads whole values by their leaves,
    /// and a box is read back exactly as it was written.
    pub(crate) fn boxed_size(&mut self, ty: &Type) -> Value {
        let bytes =
            i64::from(leaf_count(ty, self.program, self.pointer)) * i64::from(RETURN_SLOT_STRIDE);
        self.builder.ins().iconst(self.pointer, bytes)
    }

    /// `box(h, value)` — one `malloc`, and the value written into it (§3).
    pub(crate) fn boxed(&mut self, ty: &Type, value: &Expr) -> Value {
        let values = self.expr(value);
        let pointer = self.pointer;
        let size = self.boxed_size(ty);
        let id = self.libc_fn("malloc", &[pointer], &[pointer]);
        let f = self.module.declare_func_in_func(id, self.builder.func);
        let call = self.builder.ins().call(f, &[size]);
        let at = self.builder.inst_results(call)[0];
        // Out of memory traps, exactly as an exhausted arena does. A null
        // pointer wandering into the store below is undefined behaviour, and
        // this language does not have any to wander into.
        self.builder.ins().trapz(at, TrapCode::HEAP_OUT_OF_BOUNDS);
        self.store_leaves(at, &values);
        at
    }

    /// `unbox(h, b)` — read the value back, then one `free` (§3).
    ///
    /// The load has to happen *before* the free, which is the only ordering
    /// constraint in the whole heap and is why this is one function rather
    /// than two composable ones.
    pub(crate) fn unboxed(&mut self, ty: &Type, value: &Expr) -> Vec<Value> {
        let at = self.expr(value)[0];
        let kinds = leaves(ty, self.program, self.pointer);
        let values = self.load_leaves(at, &kinds);
        self.free(at);
        values
    }
}
