//! `Fs` (`docs/filesystem.md` §3-4, `docs/file-handles.md`): `fs_read`/
//! `fs_write`, `open_read`, `file_read` and `file_close`.

use crate::*;

impl<'a> FuncEmitter<'a> {
    /// libc's `errno`, read through the per-thread accessor `emit.rs`
    /// declared -- `__errno_location` on glibc, `__error` on Darwin,
    /// both answering a pointer to it. Mirrors `lex-sys-codegen`'s own
    /// `errno` (`body/memory.rs`).
    fn errno(&mut self) -> LValue {
        let symbol = match self.triple.operating_system {
            target_lexicon::OperatingSystem::Darwin(_) => "__error",
            _ => "__errno_location",
        };
        let addr = self.fresh();
        self.out.push_str(&format!("  {addr} = call ptr @{symbol}()\n"));
        let value32 = self.fresh();
        self.out.push_str(&format!("  {value32} = load i32, ptr {addr}\n"));
        let value = self.fresh();
        self.out.push_str(&format!("  {value} = sext i32 {value32} to i64\n"));
        LValue::Reg(value)
    }

    /// The longest path a file operation will build, including the NUL
    /// -- copied onto the stack to terminate it for C, since a slice
    /// carries no terminator of its own (`docs/strings.md` §6). A
    /// longer path traps rather than being cut short, because a
    /// silently truncated path names a different file. Mirrors
    /// `lex-sys-codegen`'s own `checked_path` (`body/memory.rs`) line
    /// for line; the loop is built the same "no `phi`" way `connect`'s
    /// own `checked_host` already is.
    fn checked_path(&mut self, prefix: &str, path: &[LValue]) -> Result<String, String> {
        const PATH_MAX: i64 = 4096;
        let (source, length) = (operand(&path[0]), operand(&path[1]));

        let too_long = self.fresh();
        self.out.push_str(&format!("  {too_long} = icmp uge i64 {length}, {PATH_MAX}\n"));
        self.trap_if(&too_long)?;

        let short = self.fresh();
        self.out.push_str(&format!("  {short} = icmp ult i64 {length}, {}\n", prefix.len()));
        self.trap_if(&short)?;

        let buffer = self.fresh();
        self.out.push_str(&format!("  {buffer} = alloca i8, i64 {PATH_MAX}\n"));
        let expected = operand(&self.bytes_lit(prefix)[0]);

        let cursor = self.fresh();
        self.out.push_str(&format!("  {cursor} = alloca i64\n"));
        self.out.push_str(&format!("  store i64 0, ptr {cursor}\n"));
        let previous = self.fresh();
        self.out.push_str(&format!("  {previous} = alloca i8\n"));
        self.out.push_str(&format!("  store i8 0, ptr {previous}\n"));

        let n = self.blocks;
        self.blocks += 1;
        let (header, body, done) =
            (format!("pathhdr{n}"), format!("pathbody{n}"), format!("pathdone{n}"));
        self.out.push_str(&format!("  br label %{header}\n"));

        self.out.push_str(&format!("{header}:\n"));
        let i = self.fresh();
        self.out.push_str(&format!("  {i} = load i64, ptr {cursor}\n"));
        let more = self.fresh();
        self.out.push_str(&format!("  {more} = icmp ult i64 {i}, {length}\n"));
        self.out.push_str(&format!("  br i1 {more}, label %{body}, label %{done}\n"));

        self.out.push_str(&format!("{body}:\n"));
        let at = self.fresh();
        self.out.push_str(&format!("  {at} = getelementptr i8, ptr {source}, i64 {i}\n"));
        let byte = self.fresh();
        self.out.push_str(&format!("  {byte} = load i8, ptr {at}\n"));
        let into = self.fresh();
        self.out.push_str(&format!("  {into} = getelementptr i8, ptr {buffer}, i64 {i}\n"));
        self.out.push_str(&format!("  store i8 {byte}, ptr {into}\n"));

        // `..` is refused rather than normalised (`docs/filesystem.md`
        // §4.1): normalising is a security function with its own
        // design, and a prefix check that quietly admitted
        // `/tmp/../etc` would be the dishonest third option.
        let dot = self.fresh();
        self.out.push_str(&format!("  {dot} = icmp eq i8 {byte}, 46\n"));
        let prior = self.fresh();
        self.out.push_str(&format!("  {prior} = load i8, ptr {previous}\n"));
        let prior_dot = self.fresh();
        self.out.push_str(&format!("  {prior_dot} = icmp eq i8 {prior}, 46\n"));
        let traversal = self.fresh();
        self.out.push_str(&format!("  {traversal} = and i1 {dot}, {prior_dot}\n"));
        self.trap_if(&traversal)?;
        self.out.push_str(&format!("  store i8 {byte}, ptr {previous}\n"));

        // Inside the prefix, the bytes have to match. A path outside
        // what the capability granted is a broken promise, so it traps
        // rather than returning `-1`.
        let inside = self.fresh();
        self.out.push_str(&format!("  {inside} = icmp ult i64 {i}, {}\n", prefix.len()));
        let want_at = self.fresh();
        self.out.push_str(&format!("  {want_at} = getelementptr i8, ptr {expected}, i64 {i}\n"));
        let want = self.fresh();
        self.out.push_str(&format!("  {want} = load i8, ptr {want_at}\n"));
        let differs = self.fresh();
        self.out.push_str(&format!("  {differs} = icmp ne i8 {byte}, {want}\n"));
        let escaped = self.fresh();
        self.out.push_str(&format!("  {escaped} = and i1 {inside}, {differs}\n"));
        self.trap_if(&escaped)?;

        let next = self.fresh();
        self.out.push_str(&format!("  {next} = add i64 {i}, 1\n"));
        self.out.push_str(&format!("  store i64 {next}, ptr {cursor}\n"));
        self.out.push_str(&format!("  br label %{header}\n"));

        self.out.push_str(&format!("{done}:\n"));
        let end = self.fresh();
        self.out.push_str(&format!("  {end} = getelementptr i8, ptr {buffer}, i64 {length}\n"));
        self.out.push_str(&format!("  store i8 0, ptr {end}\n"));

        // The bytes matching is not yet the same as being inside the
        // directory: `/tmp` does not contain `/tmpevil`. The path
        // either *is* the granted name or continues from it at a `/`.
        if !prefix.is_empty() && !prefix.ends_with('/') {
            let at = self.fresh();
            self.out.push_str(&format!(
                "  {at} = getelementptr i8, ptr {buffer}, i64 {}\n",
                prefix.len()
            ));
            let byte = self.fresh();
            self.out.push_str(&format!("  {byte} = load i8, ptr {at}\n"));
            let separator = self.fresh();
            self.out.push_str(&format!("  {separator} = icmp eq i8 {byte}, 47\n"));
            let ended = self.fresh();
            self.out.push_str(&format!("  {ended} = icmp eq i8 {byte}, 0\n"));
            let boundary = self.fresh();
            self.out.push_str(&format!("  {boundary} = or i1 {separator}, {ended}\n"));
            let not_boundary = self.fresh();
            self.out.push_str(&format!("  {not_boundary} = xor i1 {boundary}, true\n"));
            self.trap_if(&not_boundary)?;
        }

        Ok(buffer)
    }

    /// `fs_read`/`fs_write` (`docs/filesystem.md` §3). Neither call is
    /// `open(path, flags, mode)`: `open` is variadic, and on Apple
    /// ARM64 a variadic argument is passed on the stack while a fixed
    /// one is passed in a register, so a fixed three-argument
    /// declaration would put `mode` where the callee never looks. So:
    /// `creat(path, mode)` to write, `open(path, O_RDONLY)` to read,
    /// both non-variadic, the same choice
    /// `lex-sys-codegen`'s own `file_op` already made and
    /// `docs/filesystem.md` §2.2 explains.
    pub(crate) fn file_op(
        &mut self,
        write: bool,
        prefix: &str,
        args: &[Expr],
    ) -> Result<Vec<LValue>, String> {
        let path = self.expr(&args[1])?;
        let bytes = self.expr(&args[2])?;
        let path = self.checked_path(prefix, &path)?;

        let fd = self.fresh();
        if write {
            self.out.push_str(&format!("  {fd} = call i32 @creat(ptr {path}, i32 420)\n"));
        } else {
            self.out.push_str(&format!("  {fd} = call i32 @open(ptr {path}, i32 0)\n"));
        }

        let result_cell = self.fresh();
        self.out.push_str(&format!("  {result_cell} = alloca i64\n"));

        let bad = self.fresh();
        self.out.push_str(&format!("  {bad} = icmp slt i32 {fd}, 0\n"));
        let n = self.blocks;
        self.blocks += 1;
        let (failed, opened, merge) =
            (format!("fileopfailed{n}"), format!("fileopopened{n}"), format!("fileopmerge{n}"));
        self.out.push_str(&format!("  br i1 {bad}, label %{failed}, label %{opened}\n"));

        self.out.push_str(&format!("{failed}:\n"));
        self.out.push_str(&format!("  store i64 -1, ptr {result_cell}\n"));
        self.out.push_str(&format!("  br label %{merge}\n"));

        self.out.push_str(&format!("{opened}:\n"));
        let name = if write { "write" } else { "read" };
        let moved = self.fresh();
        self.out.push_str(&format!(
            "  {moved} = call i64 @{name}(i32 {fd}, ptr {}, i64 {})\n",
            operand(&bytes[0]),
            operand(&bytes[1])
        ));
        self.out.push_str(&format!("  call i32 @close(i32 {fd})\n"));
        self.out.push_str(&format!("  store i64 {moved}, ptr {result_cell}\n"));
        self.out.push_str(&format!("  br label %{merge}\n"));

        self.out.push_str(&format!("{merge}:\n"));
        let result = self.fresh();
        self.out.push_str(&format!("  {result} = load i64, ptr {result_cell}\n"));
        Ok(vec![LValue::Reg(result)])
    }

    /// `open_read(fs, path)` (`docs/file-handles.md` §2.1): the first
    /// half of [`Self::file_op`] and then it stops -- the same prefix
    /// check, the same non-variadic `open(path, O_RDONLY)`, and the
    /// descriptor *kept* rather than spent on one transfer and closed.
    /// What comes back is `Opened`'s three leaves: the tag, `Ok`'s
    /// descriptor, `Failed`'s reason.
    pub(crate) fn open_file(&mut self, prefix: &str, args: &[Expr]) -> Result<Vec<LValue>, String> {
        let path = self.expr(&args[1])?;
        let path = self.checked_path(prefix, &path)?;

        let fd32 = self.fresh();
        self.out.push_str(&format!("  {fd32} = call i32 @open(ptr {path}, i32 0)\n"));
        let fd = self.fresh();
        self.out.push_str(&format!("  {fd} = sext i32 {fd32} to i64\n"));

        let failed = self.fresh();
        self.out.push_str(&format!("  {failed} = icmp slt i64 {fd}, 0\n"));
        let tag = self.fresh();
        self.out.push_str(&format!("  {tag} = select i1 {failed}, i64 1, i64 0\n"));
        let reason = self.errno();
        Ok(vec![LValue::Reg(tag), LValue::Reg(fd), reason])
    }

    /// `file_read(file, into)` (`docs/file-handles.md` §3): one
    /// `read(2)`, its three outcomes sorted into `Read`'s three
    /// constructors. `args[0]` is the handle's own address -- a
    /// reference, one pointer leaf, so the descriptor is a load rather
    /// than the value itself, unlike `file_close` below.
    pub(crate) fn read_file(&mut self, args: &[LValue]) -> Result<Vec<LValue>, String> {
        let fd64 = self.fresh();
        self.out.push_str(&format!("  {fd64} = load i64, ptr {}\n", operand(&args[0])));
        let fd = self.fresh();
        self.out.push_str(&format!("  {fd} = trunc i64 {fd64} to i32\n"));

        let moved = self.fresh();
        self.out.push_str(&format!(
            "  {moved} = call i64 @read(i32 {fd}, ptr {}, i64 {})\n",
            operand(&args[1]),
            operand(&args[2])
        ));

        let negative = self.fresh();
        self.out.push_str(&format!("  {negative} = icmp slt i64 {moved}, 0\n"));
        let empty = self.fresh();
        self.out.push_str(&format!("  {empty} = icmp eq i64 {moved}, 0\n"));
        let not_failed = self.fresh();
        self.out.push_str(&format!("  {not_failed} = select i1 {empty}, i64 1, i64 0\n"));
        let tag = self.fresh();
        self.out.push_str(&format!("  {tag} = select i1 {negative}, i64 2, i64 {not_failed}\n"));
        let reason = self.errno();
        Ok(vec![LValue::Reg(tag), LValue::Reg(moved), reason])
    }
}
