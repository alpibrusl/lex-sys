//! Lowering `cancho_ir::Program` to LLVM textual IR.
//!
//! Every cancho value is its leaves, exactly as `cancho-codegen`'s own
//! `abi.rs` scalarises one: a capability is zero leaves, a reference is one
//! pointer leaf (whatever it refers to), `int` is one `i64`. Unlike the
//! Cranelift path, which builds SSA `Variable`s that Cranelift itself
//! promotes out of memory, every leaf here is one `alloca`, written and
//! read with plain `store`/`load` -- `clang`'s mandatory `mem2reg` does the
//! promotion this crate does not bother building, which is the one place
//! shelling out to `clang` buys more than a linker.
//!
//! A failure returns `(Option<usize>, String)` -- the function index and a
//! message -- exactly the shape [`cancho_codegen::CodegenError`] wants,
//! without this crate depending on Cranelift to build it.
//!
//! `mem2reg` is not automatic: it runs as part of `clang`'s standard `-O1+`
//! pipeline, not at the default `-O0` (`docs/llvm-backend.md` §7 measured
//! the difference and corrected the earlier "mandatory" claim). `lib.rs`'s
//! `run_clang` always passes `-O2` for exactly this reason -- without it,
//! every leaf here stays a real stack slot.

use cancho_ir::Program;
use cancho_types::Type;
use target_lexicon::Triple;

use crate::*;

/// One arena's chunk, matching `cancho-codegen`'s own `abi::ARENA_CHUNK`
/// exactly (§7.5): some `benches/` programs (`sieve_checked.cho`'s own
/// header) size their allocation against this constant, so a mismatched
/// chunk size would trap where the Cranelift build does not, or the
/// reverse.
pub(crate) const ARENA_CHUNK: i64 = 64 * 1024;

/// Every NaN's canonical `bits_of` answer, matching `cancho-codegen`'s
/// own `abi::CANONICAL_NAN` exactly (§7.17): x86-64 and aarch64 disagree
/// on the sign bit a generated NaN (`0.0 / 0.0`) carries, so `bits_of`
/// canonicalises every NaN to this one pattern rather than letting the
/// answer depend on where the program ran.
pub(crate) const CANONICAL_NAN: i64 = 0x7ff8_0000_0000_0000;

/// `bits_of32`'s canonical NaN, matching `cancho-codegen`'s own
/// `abi::CANONICAL_NAN_32` (`docs/f32.md` §2).
pub(crate) const CANONICAL_NAN_32: i64 = 0x7fc0_0000;

/// `2^63` as a `float`, both signs -- `truncate`'s own trap bound
/// (`docs/floating-point.md` §4: "any magnitude at or beyond `2^63`",
/// which includes exactly `-2^63` even though it is a representable
/// `i64`, the same choice Cranelift's `fcvt_to_sint` makes to sidestep
/// x86's `cvttsd2si` "integer indefinite" pattern colliding with a
/// legitimate `int::MIN`). Printed in hex float syntax for the same
/// bit-exactness reason `LValue::FConst` is.
pub(crate) const TRUNCATE_UPPER_BOUND: &str = "0x43E0000000000000";
pub(crate) const TRUNCATE_LOWER_BOUND: &str = "0xC3E0000000000000";

/// `docs/reach.md` §3: a capability parameter carries no data at run
/// time, so it never crosses to C. Mirrors `cancho-codegen`'s own
/// `abi::crosses_to_c` exactly -- the one exception is a `&r [byte]`
/// reference, which crosses as the pointer-and-length pair every other
/// slice leaf already is.
pub(crate) fn crosses_to_c(ty: &Type) -> bool {
    match ty {
        Type::Ref { inner, .. } => {
            matches!(inner.as_ref(), Type::Slice(element) if **element == Type::Byte)
        }
        _ => true,
    }
}

/// The machine types a leaf may be. `F64` is `docs/floating-point.md`'s
/// `float` -- binary64, one leaf, exactly like `int` (§7.17).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LKind {
    I64,
    I8,
    Ptr,
    F64,
    /// `docs/f32.md`'s `f32` -- binary32, one leaf.
    F32,
}

impl LKind {
    pub(crate) fn llvm(self) -> &'static str {
        match self {
            LKind::I64 => "i64",
            LKind::I8 => "i8",
            LKind::Ptr => "ptr",
            LKind::F64 => "double",
            LKind::F32 => "float",
        }
    }

    pub(crate) fn zero(self) -> &'static str {
        match self {
            LKind::I64 | LKind::I8 => "0",
            LKind::Ptr => "null",
            // Exact: zero is the one float value with a trivial bit
            // pattern, so it round-trips through decimal with nothing
            // lost, unlike every other float constant here (`FConst`
            // below).
            LKind::F64 | LKind::F32 => "0.0",
        }
    }
}

/// An operand: a compile-time constant, printed inline, or a named
/// register a prior instruction produced. LLVM textual IR allows a
/// constant wherever a register is expected, so a literal never needs an
/// instruction of its own the way Cranelift's `iconst` does.
#[derive(Clone)]
pub(crate) enum LValue {
    Const(i64),
    /// A `float` constant, stored as its raw bits -- `Expr::Float`'s own
    /// shape (`docs/floating-point.md` §1) -- and printed in LLVM's hex
    /// float syntax rather than decimal, so the constant this backend
    /// emits is the bit pattern the parser read, not whatever a decimal
    /// round-trip through `f64`'s `Display` happens to preserve.
    FConst(u64),
    /// An `f32` constant, as its binary32 bits (`docs/f32.md` §2).
    /// LLVM's hex syntax for a `float` constant is the *double* with the
    /// same value, which every binary32 is exactly, so this prints
    /// `f32 -> f64` bits and nothing is rounded.
    F32Const(u32),
    Reg(String),
}

pub(crate) fn operand(v: &LValue) -> String {
    match v {
        LValue::Const(n) => n.to_string(),
        LValue::FConst(bits) => format!("0x{bits:016X}"),
        LValue::F32Const(bits) => format!("0x{:016X}", f64::from(f32::from_bits(*bits)).to_bits()),
        LValue::Reg(name) => name.clone(),
    }
}

/// The LLVM type a multi-leaf value returns as: an anonymous struct, one
/// field per leaf -- the same shape `checked_arith`'s own `{i64, i1}`
/// already is for LLVM's overflow intrinsics, applied here to a
/// `cancho` value with more than one leaf (`docs/llvm-backend.md`
/// §7.7: `contents(b)`'s own two leaves, called through a function like
/// `reduce_checked.cho`'s `fill`, is what this exists for).
pub(crate) fn struct_ty(kinds: &[LKind]) -> String {
    format!("{{{}}}", kinds.iter().map(|k| k.llvm()).collect::<Vec<_>>().join(", "))
}

/// The leaves a type scalarises to, matching `cancho-codegen`'s
/// `abi::leaves_into` for the subset this slice supports.
///
/// `Err` names what is missing rather than panicking: a type this function
/// refuses is a type the first slice does not lower yet, not a checker bug.
pub(crate) fn leaves_of(ty: &Type, program: &Program) -> Result<Vec<LKind>, String> {
    let mut out = Vec::new();
    leaves_into(ty, program, &mut out)?;
    Ok(out)
}

fn leaves_into(ty: &Type, program: &Program, out: &mut Vec<LKind>) -> Result<(), String> {
    match ty {
        Type::Int => out.push(LKind::I64),
        // `docs/opaque-pointers.md` §4: LLVM's own `ptr` kind, not `i64`
        // -- an opaque handle is never arithmetic here, so there is no
        // reason to spell it as an integer the way Cranelift's pointer-
        // width leaf does; `LKind::Ptr`'s `zero()` is already `"null"`,
        // exactly the sentinel `null_ptr()` needs.
        Type::CPtr => out.push(LKind::Ptr),
        // `docs/function-values.md` §4.2: `val`, one leaf, a pointer to
        // the target's compiled entry point -- LLVM's own `ptr` kind,
        // the same one `Type::CPtr` and a reference's own leaf use.
        Type::Fn(..) => out.push(LKind::Ptr),
        Type::Byte | Type::Bool => out.push(LKind::I8),
        Type::Float => out.push(LKind::F64),
        Type::F32 => out.push(LKind::F32),
        Type::Ref { inner, .. } => {
            out.push(LKind::Ptr);
            if matches!(inner.as_ref(), Type::Slice(_)) {
                out.push(LKind::I64);
            }
        }
        Type::Tuple(parts) => {
            for part in parts {
                leaves_into(part, program, out)?;
            }
        }
        // `docs/file-handles.md`: a handle at run time is a descriptor
        // and nothing else -- one leaf, where the six capabilities are
        // zero. Matches `cancho-codegen`'s own `abi::leaves_into`
        // exactly; `File` is a prelude type, not an ordinary struct
        // `program.type_info` would scalarise correctly on its own --
        // `Opened`/`Read` are ordinary prelude *enums* instead, and
        // fall through to the general `TypeInfo::Enum` arm below
        // unaided, the same way they do for Cranelift.
        Type::Named(def, _)
            if matches!(
                def.0 as usize,
                cancho_ir::PRELUDE_FILE
                    | cancho_ir::PRELUDE_DIR
                    | cancho_ir::PRELUDE_DIR_LIST
                    | cancho_ir::PRELUDE_LISTENER
                    | cancho_ir::PRELUDE_CONN
                    | cancho_ir::PRELUDE_UDP
                    // `docs/tty.md` §3, edition 8: `Udp`'s shape.
                    | cancho_ir::PRELUDE_PORT
                    | cancho_ir::PRELUDE_POLLER
                    | cancho_ir::PRELUDE_SIGNAL_WATCH
                    // `docs/processes.md` §3.1: a pid, and two descriptors.
                    | cancho_ir::PRELUDE_CHILD
                    | cancho_ir::PRELUDE_PIPE
                    | cancho_ir::PRELUDE_CHILD_END
            ) =>
        {
            out.push(LKind::I64);
        }
        // `docs/heap.md` §3: a box at run time is a pointer and nothing
        // else -- no header, no refcount, no tag -- except a box of an
        // *unsized* referent, which carries the length too, because
        // nothing else knows how many elements there are (`docs/boxed-
        // slices.md` §2, the same pair `&r [T]` already is). Matches
        // `cancho-codegen`'s own `abi::leaves_into` exactly; `Box` is a
        // prelude type, not an ordinary struct `program.type_info` would
        // scalarise correctly on its own.
        Type::Named(def, args) if def.0 as usize == cancho_ir::PRELUDE_BOX => {
            out.push(LKind::Ptr);
            if matches!(args.first(), Some(Type::Slice(_))) {
                out.push(LKind::I64);
            }
        }
        // `docs/threads.md` §2: a `Thread[T, R]` at run time is a real
        // `pthread_t`, one leaf -- the same "no fields on purpose" shape
        // `Box` already has above. Matches `cancho-codegen`'s own
        // `abi::leaves_into` exactly.
        Type::Named(def, _) if def.0 as usize == cancho_ir::PRELUDE_THREAD => {
            out.push(LKind::Ptr);
        }
        Type::Named(def, args) => match program.type_info(*def) {
            cancho_ir::TypeInfo::Struct { fields, .. } => {
                for (_, field) in fields {
                    leaves_into(&field.substitute(args, &[]), program, out)?;
                }
            }
            // A tag, then *every* variant's payload leaves -- wasteful and
            // deliberately so, matching `cancho-codegen`'s own rule
            // (`abi::leaves_into`): overlaying the payloads is a layout
            // decision no M1 backend makes. The tag is an `i64` for the
            // same reason -- picking a narrower integer would be one too.
            cancho_ir::TypeInfo::Enum { variants, .. } => {
                out.push(LKind::I64);
                for (_, payload) in variants {
                    for ty in payload {
                        leaves_into(&ty.substitute(args, &[]), program, out)?;
                    }
                }
            }
        },
        other => {
            return Err(format!(
                "`{other:?}` is not part of the LLVM backend's first slice (docs/llvm-backend.md §5)"
            ));
        }
    }
    Ok(())
}

pub(crate) fn emit_module(
    program: &Program,
    entry: &str,
    triple: &Triple,
) -> Result<String, (Option<usize>, String)> {
    // The tables in `cancho_ir` (open flags, `stat`, `dirent`, errno) know
    // three operating systems. Say so here, once, rather than let a fourth
    // fall through to the Linux numbers: `x86_64-unknown-freebsd` would
    // otherwise build, and be wrong in a way no linker notices.
    {
        use target_lexicon::OperatingSystem as Os;
        if !matches!(
            triple.operating_system,
            Os::Linux | Os::Darwin(_) | Os::Wasi | Os::WasiP1 | Os::WasiP2
        ) {
            return Err((
                None,
                format!(
                    "the LLVM backend has constant tables for Linux, Darwin and WASI only, not \
                     `{}` (docs/wasm.md)",
                    triple.operating_system
                ),
            ));
        }
    }
    let mut text = String::new();
    text.push_str(&format!("target triple = \"{triple}\"\n\n"));
    // C's `size_t`: 4 bytes on wasm32, 8 everywhere else this backend targets.
    // Every libc declaration below that takes or answers a size says `{st}`, and
    // every call site uses `FuncEmitter::size_ty` for the same answer (`docs/wasm.md`).
    let st = size_ty(triple);
    text.push_str("declare i32 @putchar(i32)\n");
    // `docs/standard-input.md` §3 (§7.9): the mirror. `int getchar(void)`
    // -- no parameter, and the same `i32` result sign-extended at the
    // edge, which is what carries `EOF`'s `-1` back as `-1` rather than
    // as a very large unsigned number were it zero-extended instead.
    text.push_str("declare i32 @getchar()\n");
    // `docs/bulk-io.md` §3 (§7.11): the whole slice in one call, through
    // the same stdio stream `putchar` uses. `stdout`/`stderr` are `FILE
    // *` *variables* in libc, so the symbol is the address of the
    // pointer and the stream itself is one load away -- and the symbol
    // differs by platform, the same split `cancho-codegen`'s own
    // `emit.rs` already makes for it.
    text.push_str(&format!("declare {st} @fwrite(ptr, {st}, {st}, ptr)\n"));
    let (stdout_symbol, stderr_symbol) = match triple.operating_system {
        target_lexicon::OperatingSystem::Darwin(_) => ("__stdoutp", "__stderrp"),
        _ => ("stdout", "stderr"),
    };
    text.push_str(&format!("@{stdout_symbol} = external global ptr\n"));
    text.push_str(&format!("@{stderr_symbol} = external global ptr\n"));
    // `region`/`alloc_slice` (§7.5): one `malloc` per arena, one `free` on
    // the way out -- `cancho-codegen`'s own `body/memory.rs` `libc_fn`
    // declares these the same way, on first use rather than unconditionally
    // there, but an unused `declare` here costs nothing, the same reasoning
    // `putchar`'s own unconditional declaration already relies on.
    text.push_str(&format!("declare ptr @malloc({st})\n"));
    text.push_str("declare void @free(ptr)\n");
    // Checked arithmetic (§5's second slice): the three overflow-reporting
    // intrinsics `Expr::Bin`'s `Add`/`Sub`/`Mul` arms call. Declared
    // unconditionally, the same way `putchar` is -- an unused `declare`
    // costs nothing, and every function in the module shares one `.ll`.
    text.push_str("declare {i64, i1} @llvm.sadd.with.overflow.i64(i64, i64)\n");
    text.push_str("declare {i64, i1} @llvm.ssub.with.overflow.i64(i64, i64)\n");
    text.push_str("declare {i64, i1} @llvm.smul.with.overflow.i64(i64, i64)\n");
    // `sqrt` (§7.17, `docs/float-math.md` §2): correctly rounded per
    // IEEE-754, which is why this is the one arithmetic builtin that is
    // an intrinsic rather than an instruction sequence.
    text.push_str("declare double @llvm.sqrt.f64(double)\n");
    text.push_str("declare float @llvm.sqrt.f32(float)\n");

    // Every libc symbol from here down is one a program predating `Net`/
    // `Fs` (`examples/serve/`, `examples/vsock/`, and siblings) may
    // declare for itself through `extern fn`, at cancho's own crossing
    // widths (`docs/reach.md` §3) rather than libc's true ones -- exactly
    // the collision `read`/`write` were first guarded against
    // (`tests/accept/bytes_to_c.cho`, this backend's #117). Declaring
    // unconditionally, the way `putchar`/`malloc` safely do, made
    // `--backend llvm` refuse every socket-declaring example the moment
    // it became the default (`docs/llvm-backend.md`'s own long-recorded
    // "already-accepted exposure," #92's Cranelift equivalent) -- not
    // hypothetical once this is what `build` reaches without a flag. So
    // every one of these is now guarded the same way: a program naming
    // the symbol itself is trusted to have declared the signature it
    // actually needs, and this backend does not also insist on its own.
    let extern_symbols: std::collections::BTreeSet<&str> =
        program.externs.iter().map(|e| e.symbol.as_str()).collect();
    let declare_libc_unless_own = |text: &mut String, symbol: &str, signature: &str| {
        if !extern_symbols.contains(symbol) {
            text.push_str(&format!("declare {signature}\n"));
        }
    };

    // `listen`/`accept` (`docs/net.md` §7.20, `docs/listen.md` §6):
    // neither takes a capability -- the port was already bound at
    // `bind`.
    // `docs/zeroed-slices.md`: a `box_slice` filled with zero. Guarded like
    // the rest: `calloc` is a name a program may declare for itself.
    declare_libc_unless_own(&mut text, "calloc", &format!("ptr @calloc({st}, {st})"));
    declare_libc_unless_own(&mut text, "listen", "i32 @listen(i32, i32)");
    declare_libc_unless_own(&mut text, "accept", "i32 @accept(i32, ptr, ptr)");
    // Linux's close-on-exec accept (`docs/processes.md` §4.5); Darwin has
    // none and never calls it.
    declare_libc_unless_own(&mut text, "accept4", "i32 @accept4(i32, ptr, ptr, i32)");
    // `docs/processes.md` §6: a channel, the spawn and what it is configured
    // with, and the two verbs on a child. None of these is variadic.
    declare_libc_unless_own(&mut text, "socketpair", "i32 @socketpair(i32, i32, i32, ptr)");
    for (symbol, signature) in [
        ("posix_spawn", "i32 @posix_spawn(ptr, ptr, ptr, ptr, ptr, ptr)"),
        ("posix_spawn_file_actions_init", "i32 @posix_spawn_file_actions_init(ptr)"),
        ("posix_spawn_file_actions_destroy", "i32 @posix_spawn_file_actions_destroy(ptr)"),
        (
            "posix_spawn_file_actions_addopen",
            "i32 @posix_spawn_file_actions_addopen(ptr, i32, ptr, i32, i32)",
        ),
        (
            "posix_spawn_file_actions_adddup2",
            "i32 @posix_spawn_file_actions_adddup2(ptr, i32, i32)",
        ),
        ("posix_spawnattr_init", "i32 @posix_spawnattr_init(ptr)"),
        ("posix_spawnattr_destroy", "i32 @posix_spawnattr_destroy(ptr)"),
        ("posix_spawnattr_setflags", "i32 @posix_spawnattr_setflags(ptr, i16)"),
        ("posix_spawnattr_setsigmask", "i32 @posix_spawnattr_setsigmask(ptr, ptr)"),
        ("posix_spawnattr_setsigdefault", "i32 @posix_spawnattr_setsigdefault(ptr, ptr)"),
        (
            "posix_spawn_file_actions_addfchdir_np",
            "i32 @posix_spawn_file_actions_addfchdir_np(ptr, i32)",
        ),
        (
            "posix_spawn_file_actions_addclosefrom_np",
            "i32 @posix_spawn_file_actions_addclosefrom_np(ptr, i32)",
        ),
        ("sigemptyset", "i32 @sigemptyset(ptr)"),
        ("sigfillset", "i32 @sigfillset(ptr)"),
        ("waitpid", "i32 @waitpid(i32, ptr, i32)"),
        ("syscall", "i64 @syscall(i64, ...)"),
        ("kill", "i32 @kill(i32, i32)"),
        ("strncmp", &format!("i32 @strncmp(ptr, ptr, {st})")),
    ] {
        declare_libc_unless_own(&mut text, symbol, signature);
    }
    // `bind` (§7.21, `docs/listen.md` §6): `socket`+`setsockopt`+`bind`
    // folded into one call, the same libc surface `examples/serve/
    // serve.cho` reaches by hand and `cancho-codegen`'s own `body/net.rs`
    // already declares for Cranelift.
    declare_libc_unless_own(&mut text, "socket", "i32 @socket(i32, i32, i32)");
    declare_libc_unless_own(&mut text, "setsockopt", "i32 @setsockopt(i32, i32, i32, ptr, i32)");
    // `conn_connect_status` (`docs/native-sockets.md` §10.6) reads `SO_ERROR`.
    declare_libc_unless_own(&mut text, "getsockopt", "i32 @getsockopt(i32, i32, i32, ptr, ptr)");
    declare_libc_unless_own(&mut text, "bind", "i32 @bind(i32, ptr, i32)");
    // `udp_local_port` (`docs/udp.md` §3) reads the port the kernel chose.
    declare_libc_unless_own(&mut text, "getsockname", "i32 @getsockname(i32, ptr, ptr)");
    // `conn_peer` (`docs/conn-peer.md` §3) reads the other end's address.
    declare_libc_unless_own(&mut text, "getpeername", "i32 @getpeername(i32, ptr, ptr)");
    declare_libc_unless_own(&mut text, "close", "i32 @close(i32)");
    // `docs/directory-handles.md`: `openat` is variadic (its `mode`, read
    // with `O_CREAT`) and declared so, which puts `mode` wherever the target
    // passes a variadic argument -- the stack on Apple AArch64.
    declare_libc_unless_own(&mut text, "openat", "i32 @openat(i32, ptr, i32, ...)");
    // `dir_list`, `dir_next`, `dir_list_close` (`docs/directory-listing.md`).
    declare_libc_unless_own(&mut text, "fdopendir", "ptr @fdopendir(i32)");
    declare_libc_unless_own(&mut text, "readdir", "ptr @readdir(ptr)");
    declare_libc_unless_own(&mut text, "closedir", "i32 @closedir(ptr)");
    declare_libc_unless_own(&mut text, "fstatat", "i32 @fstatat(i32, ptr, ptr, i32)");
    // `dir_own_mode` (`docs/directory-listing.md` §3.5).
    declare_libc_unless_own(&mut text, "fstat", "i32 @fstat(i32, ptr)");
    declare_libc_unless_own(&mut text, "renameat", "i32 @renameat(i32, ptr, i32, ptr)");
    // `dir_rename_new` (`docs/directory-handles.md` §3, slice 4): each target's
    // no-replace rename; the other target's is never called.
    declare_libc_unless_own(&mut text, "renameat2", "i32 @renameat2(i32, ptr, i32, ptr, i32)");
    declare_libc_unless_own(
        &mut text,
        "renameatx_np",
        "i32 @renameatx_np(i32, ptr, i32, ptr, i32)",
    );
    declare_libc_unless_own(&mut text, "unlinkat", "i32 @unlinkat(i32, ptr, i32)");
    // `connect` (§7.22, `docs/connect.md` §10): the last of `Net`'s four
    // builtins, needing `getaddrinfo`/`freeaddrinfo` (host resolution)
    // and `connect` itself alongside the `socket` already declared above.
    declare_libc_unless_own(&mut text, "getaddrinfo", "i32 @getaddrinfo(ptr, ptr, ptr, ptr)");
    declare_libc_unless_own(&mut text, "freeaddrinfo", "void @freeaddrinfo(ptr)");
    // `docs/tty.md` §3, edition 8: the serial-port configuration calls.
    // `ioctl` is variadic; the backend emits its call shaped the way the
    // callee reads it (`native-sockets.md` §3's `fcntl` precedent), so
    // the declaration is the real variadic one.
    declare_libc_unless_own(&mut text, "tcsetattr", "i32 @tcsetattr(i32, i32, ptr)");
    declare_libc_unless_own(&mut text, "tcflush", "i32 @tcflush(i32, i32)");
    declare_libc_unless_own(&mut text, "ioctl", "i32 @ioctl(i32, i32, ...)");
    declare_libc_unless_own(&mut text, "connect", "i32 @connect(i32, ptr, i32)");
    // The socket handles (`docs/native-sockets.md` §3). `fcntl` is declared
    // variadic, which is what it is: on Apple arm64 a variadic argument is
    // passed on the stack, so a fixed-signature declaration would put the
    // flags where `fcntl` does not look.
    // `docs/native-sockets.md` §4: the poller. Only the target's own
    // facility is declared -- `epoll` on Linux, `kqueue` on Darwin.
    match triple.operating_system {
        target_lexicon::OperatingSystem::Darwin(_) => {
            declare_libc_unless_own(&mut text, "kqueue", "i32 @kqueue()");
            declare_libc_unless_own(
                &mut text,
                "kevent",
                "i32 @kevent(i32, ptr, i32, ptr, i32, ptr)",
            );
            // `docs/signals.md` section 5: a signal is ignored, then watched with `EVFILT_SIGNAL`.
            declare_libc_unless_own(&mut text, "sigaction", "i32 @sigaction(i32, ptr, ptr)");
            declare_libc_unless_own(&mut text, "raise", "i32 @raise(i32)");
        }
        _ => {
            // `docs/signals.md` section 5: block the set, read it from a `signalfd`.
            declare_libc_unless_own(
                &mut text,
                "pthread_sigmask",
                "i32 @pthread_sigmask(i32, ptr, ptr)",
            );
            declare_libc_unless_own(&mut text, "signalfd", "i32 @signalfd(i32, ptr, i32)");
            declare_libc_unless_own(&mut text, "epoll_create1", "i32 @epoll_create1(i32)");
            declare_libc_unless_own(&mut text, "epoll_ctl", "i32 @epoll_ctl(i32, i32, i32, ptr)");
            declare_libc_unless_own(&mut text, "epoll_wait", "i32 @epoll_wait(i32, ptr, i32, i32)");
        }
    }
    declare_libc_unless_own(&mut text, "clock_gettime", "i32 @clock_gettime(i32, ptr)");
    declare_libc_unless_own(&mut text, "recv", "i64 @recv(i32, ptr, i64, i32)");
    declare_libc_unless_own(&mut text, "send", "i64 @send(i32, ptr, i64, i32)");
    // `udp_recv_from` and `udp_send_to` (`docs/udp.md` §4).
    declare_libc_unless_own(&mut text, "recvfrom", "i64 @recvfrom(i32, ptr, i64, i32, ptr, ptr)");
    declare_libc_unless_own(&mut text, "sendto", "i64 @sendto(i32, ptr, i64, i32, ptr, i32)");
    declare_libc_unless_own(&mut text, "fcntl", "i32 @fcntl(i32, i32, ...)");
    text.push('\n');

    // `Fs` (§7.24, `docs/filesystem.md` §3-4, `docs/file-handles.md`):
    // `fs_read`/`fs_write` (`openat` then `read`/`write` then `close`),
    // `open_read` (`openat`, descriptor kept), `file_read`
    // (`read`) and `file_close` (`close`, already declared above for
    // `bind`'s own use). `errno`'s accessor is a *function* in every
    // modern libc -- `__errno_location` on glibc, `__error` on Darwin --
    // both answering a pointer to a thread-local `int`, the same split
    // `cancho-codegen`'s own `errno` already makes.
    declare_libc_unless_own(&mut text, "read", &format!("{st} @read(i32, ptr, {st})"));
    // `copy_within` (`docs/memory-moves.md`) and `copy_into` (`docs/bulk-copy.md`).
    declare_libc_unless_own(&mut text, "memmove", &format!("ptr @memmove(ptr, ptr, {st})"));
    // `index_of_byte` (`docs/byte-search.md`).
    declare_libc_unless_own(&mut text, "memchr", &format!("ptr @memchr(ptr, i32, {st})"));
    declare_libc_unless_own(&mut text, "write", &format!("{st} @write(i32, ptr, {st})"));
    // `docs/file-writes.md`: the write side of a file handle. `fopen`/
    // `fcntl(F_DUPFD_CLOEXEC)`/`fclose` are the opens' bridge (section 3).
    declare_libc_unless_own(&mut text, "fopen", "ptr @fopen(ptr, ptr)");
    declare_libc_unless_own(&mut text, "fileno", "i32 @fileno(ptr)");
    declare_libc_unless_own(&mut text, "fclose", "i32 @fclose(ptr)");
    declare_libc_unless_own(&mut text, "pwrite", &format!("{st} @pwrite(i32, ptr, {st}, i64)"));
    declare_libc_unless_own(&mut text, "pread", &format!("{st} @pread(i32, ptr, {st}, i64)"));
    declare_libc_unless_own(&mut text, "fsync", "i32 @fsync(i32)");
    // `flush_out` (`docs/checked-output.md`): the stream `fwrite` uses,
    // flushed, then asked whether an earlier write failed.
    declare_libc_unless_own(&mut text, "fflush", "i32 @fflush(ptr)");
    declare_libc_unless_own(&mut text, "ferror", "i32 @ferror(ptr)");
    declare_libc_unless_own(&mut text, "ftruncate", "i32 @ftruncate(i32, i64)");
    declare_libc_unless_own(&mut text, "lseek", "i64 @lseek(i32, i64, i32)");
    declare_libc_unless_own(&mut text, "unlink", "i32 @unlink(ptr)");
    declare_libc_unless_own(&mut text, "rename", "i32 @rename(ptr, ptr)");
    declare_libc_unless_own(&mut text, "flock", "i32 @flock(i32, i32)");
    let errno_symbol = match triple.operating_system {
        target_lexicon::OperatingSystem::Darwin(_) => "__error",
        _ => "__errno_location",
    };
    text.push_str(&format!("declare ptr @{errno_symbol}()\n\n"));
    if matches!(
        triple.operating_system,
        target_lexicon::OperatingSystem::Wasi
            | target_lexicon::OperatingSystem::WasiP1
            | target_lexicon::OperatingSystem::WasiP2
    ) {
        text.push_str(&wasi_errno_translation());
    }

    // `docs/threads.md` §2: `spawn`/`join`, real `pthread_create`/
    // `pthread_join`. `pthread_t` is opaque on both this project's
    // targets (glibc's own `unsigned long`, Darwin's own pointer) but
    // always one register-width value, so `ptr` -- the same kind
    // `c_ptr` already uses -- holds it either way; `pthread_create`'s
    // own `pthread_t *thread` and `pthread_join`'s own `void **retval`
    // are the same `ptr` a step further, pointing at one.
    declare_libc_unless_own(&mut text, "pthread_create", "i32 @pthread_create(ptr, ptr, ptr, ptr)");
    // Arm's data-independent-timing bit (`crate::dit`): the OS is asked
    // whether the CPU has it.
    if crate::dit::applies(triple) {
        let (symbol, signature) = crate::dit::libc_declaration(triple);
        declare_libc_unless_own(&mut text, symbol, signature);
    }
    declare_libc_unless_own(&mut text, "pthread_join", "i32 @pthread_join(ptr, ptr)");

    // `extern fn` (§7.23, §8.4): an import under the symbol the
    // declaration named. A capability parameter carries no data and
    // never reaches C (`crosses_to_c`); everything else crosses at
    // cancho's own widths -- an `int` is `i64` here whatever the C
    // function's own parameter width is, the same choice
    // `cancho-codegen`'s own `emit.rs` makes and `docs/reach.md` §3
    // documents. `Type::Unit` (no `-> Type` in the declaration) is
    // `void`; it is otherwise unwritable, so this is the one place it
    // is read rather than produced. Declared once per distinct symbol:
    // two `extern fn` declarations naming the same symbol (in different
    // modules, `docs/modules.md` §3) would otherwise redeclare it, which
    // `clang` refuses if the signatures disagree the same way Cranelift's
    // own `declare_function` already does.
    let mut declared_symbols = std::collections::BTreeSet::new();
    for ext in &program.externs {
        if !declared_symbols.insert(ext.symbol.clone()) {
            continue;
        }
        let mut params = Vec::new();
        for param in ext.params.iter().filter(|t| crosses_to_c(t)) {
            for leaf in leaves_of(param, program).map_err(|m| (None, m))? {
                params.push(leaf.llvm().to_owned());
            }
        }
        // `docs/reach.md` §3.4: `c_int` crosses at the real C ABI width
        // (32 bits), not `leaves_of`'s own `i64` -- the call site
        // (`body/expr.rs`'s `Callee::Extern` arm) sign-extends the
        // result back to this backend's own `int` after the call.
        let ret_ty = if matches!(ext.ret, Type::Unit) {
            "void".to_owned()
        } else if ext.narrow_return && matches!(ext.ret, Type::Int) {
            "i32".to_owned()
        } else {
            match leaves_of(&ext.ret, program).map_err(|m| (None, m))?.as_slice() {
                [] => "void".to_owned(),
                [k] => k.llvm().to_owned(),
                kinds => struct_ty(kinds),
            }
        };
        text.push_str(&format!("declare {ret_ty} @{}({})\n", ext.symbol, params.join(", ")));
    }
    if !program.externs.is_empty() {
        text.push('\n');
    }

    // Every `static`'s already-evaluated values (§7.25,
    // `docs/compile-time-data.md` §2) become one read-only global,
    // defined once for the whole program rather than once per reader --
    // `cancho-codegen`'s own `emit.rs` does the same, and for the same
    // reason a table is not a greeting. Stride matches `stride_of`
    // exactly (`body/memory.rs`): a `byte` packs one per byte, and
    // `int`/`bool`/`float` -- the only other element types a `static` may
    // hold (`compile-time-data.md` §4) -- pack at eight, the same
    // leaf-stride every other slice in this backend uses. An empty
    // `static` still gets a one-byte `zeroinitializer`-shaped constant,
    // for the reason an empty string literal does: a slice is a pointer
    // and a length, and the pointer has to be *some*thing.
    for data in &program.statics {
        let stride = if matches!(data.element, Type::Byte) {
            1usize
        } else {
            leaves_of(&data.element, program).map_err(|m| (None, m))?.len() * 8
        };
        let mut bytes = vec![0u8; (data.values.len() * stride).max(1)];
        for (i, value) in data.values.iter().enumerate() {
            let at = i * stride;
            bytes[at..at + stride].copy_from_slice(&value.to_le_bytes()[..stride]);
        }
        let items: Vec<String> = bytes.iter().map(|b| format!("i8 {b}")).collect();
        text.push_str(&format!(
            "@lexs_static_{} = private unnamed_addr constant [{} x i8] [{}]\n",
            data.name,
            bytes.len(),
            items.join(", ")
        ));
    }
    if !program.statics.is_empty() {
        text.push('\n');
    }

    // `arg_count`/`arg` (§7.13, `docs/arguments.md` §3): `argc`/`argv` as
    // `main` was handed them, stashed once into module-local storage and
    // never written again -- `cancho-codegen`'s own `ARGC_GLOBAL`/
    // `ARGV_GLOBAL` (`abi.rs`), the same reasoning applied to `internal
    // global` here instead of a `Linkage::Local` data object. `arg` reads
    // a NUL-terminated C string back from `argv`, so its length needs
    // libc's own `strlen` the way `docs/arguments.md` §3.2 describes.
    text.push_str(&format!("declare {st} @strlen(ptr)\n"));
    // `conn_detach`/`conn_attach`'s epoch table (`docs/native-sockets.md`
    // §10.3): a counter per descriptor, in bss.
    text.push_str(&format!(
        "@{} = internal global [{} x i32] zeroinitializer\n",
        cancho_ir::FD_EPOCH_GLOBAL,
        cancho_ir::FD_EPOCH_SLOTS
    ));
    // The peer ring (`docs/udp.md` §4): the senders `udp_recv_from` remembers,
    // then the count of tickets issued. Zero at start, in bss.
    text.push_str(&format!(
        "@{} = internal global [{} x i8] zeroinitializer, align 8\n",
        cancho_ir::UDP_PEER_GLOBAL,
        cancho_ir::UDP_PEER_BYTES
    ));
    // The signal claim's two words (`docs/signals.md` section 3): the native
    // mask of every signal a live `SignalWatch` holds, then how many spawned
    // threads have not been joined. Zero at start, in bss.
    text.push_str(&format!(
        "@{} = internal global [2 x i64] zeroinitializer\n",
        cancho_ir::SIGNAL_STATE_GLOBAL
    ));
    text.push_str("@lexs_argc = internal global i64 0\n");
    text.push_str("@lexs_argv = internal global ptr null\n\n");

    // Every string literal's bytes (§5's fourth slice) become one global
    // constant, named as it is met rather than once per unique text --
    // `docs/strings.md` §8 leaves interning an open question, so two
    // occurrences of the same literal get two objects here exactly as
    // `cancho-codegen`'s own `literals` counter gives them two. Built up
    // across every function before any of it is written into `text`,
    // because a function later in `program.funcs` may be the first one a
    // reader meets textually if `program.funcs` and source order ever
    // diverge (`docs/README.md`'s own "definition order never matters").
    let mut globals = String::new();
    let mut next_literal: u32 = 0;
    let mut bodies: Vec<String> = Vec::with_capacity(program.funcs.len());
    for (index, func) in program.funcs.iter().enumerate() {
        let body = FuncEmitter::new(program, func, triple, &mut globals, &mut next_literal)
            .and_then(|mut fe| fe.emit())
            .map_err(|message| (Some(index), message))?;
        bodies.push(body);
    }
    text.push_str(&globals);
    if !globals.is_empty() {
        text.push('\n');
    }
    for body in bodies {
        text.push_str(&body);
        text.push('\n');
    }

    let entry_id = program
        .find(entry)
        .ok_or_else(|| (None, format!("no function named `{entry}` to use as entry")))?;
    let entry_func = program.func(entry_id);
    let ret = leaves_of(&entry_func.ret, program).map_err(|m| (None, m))?;
    if ret.len() > 1 {
        return Err((None, "the entry point's return type has more than one leaf".to_owned()));
    }
    // On WebAssembly the wrapper is not `main` at all: the module defines `_start`
    // itself (`wasi_entry`) and calls this, so that the command line is fetched only
    // by a program that reads it. (A C compiler would have renamed `main(argc, argv)`
    // to `__main_argc_argv` for wasi-libc's `__main_void` to call; nothing here uses
    // `__main_void`.)
    let wasm = triple.architecture == target_lexicon::Architecture::Wasm32;
    let uses_args = wasm && crate::wasi_entry::reads_args(&text);
    let entry_header = if wasm {
        "define internal i32 @cancho_entry(i32 %argc, ptr %argv) {\n".to_owned()
    } else {
        "define i32 @main(i32 %argc, ptr %argv) {\n".to_owned()
    };
    text.push_str(&entry_header);
    text.push_str("entry:\n");
    // `docs/arguments.md` §3: written exactly once, before any cancho
    // code runs, and never again -- `cancho-codegen`'s own `emit_c_main`
    // stashes the same two values the same way, at the same point.
    text.push_str("  %argc64 = sext i32 %argc to i64\n");
    text.push_str("  store i64 %argc64, ptr @lexs_argc\n");
    text.push_str("  store ptr %argv, ptr @lexs_argv\n");
    // `crate::dit`: before any cancho code, on aarch64 Linux and Darwin.
    if crate::dit::applies(triple) {
        text.push_str("  call void @cancho_set_dit()\n");
    }
    match ret.first() {
        Some(LKind::I64) => {
            text.push_str(&format!("  %r = call i64 @lexs_{}()\n", entry_func.symbol()));
            text.push_str("  %status = trunc i64 %r to i32\n");
            // What libc does when `main` returns: write out stdout's buffer. Only
            // for a module that writes -- an unconditional flush would make every
            // pure program import `fd_write` (`wasi_console`).
            if crate::wasi_console::applies(triple) && crate::wasi_console::uses_console(&text) {
                text.push_str("  %flushed = call i32 @cancho_console_flush()\n");
            }
            text.push_str("  ret i32 %status\n");
        }
        // `docs/agent-errors.md`'s own convention applied here too: a
        // located, worded refusal rather than a silent wrong exit code.
        _ => {
            return Err((None, "`main` must return `int`, the process exit status".to_owned()));
        }
    }
    text.push_str("}\n");
    if crate::dit::applies(triple) {
        text.push_str(&crate::dit::definition(triple));
    }
    // `docs/crypto-builtins.md` §4: only the functions this module calls.
    let crypto = crate::crypto::definitions(triple, &text);
    text.push_str(&crypto);

    if triple.architecture == target_lexicon::Architecture::Wasm32 {
        text.push_str(&smul_overflow_definition());
        text.push_str(&crate::wasi_console::definitions());
        text.push_str(&crate::wasi_entry::definitions(uses_args));
    }
    Ok(text)
}

/// C's `size_t` as an LLVM type for `triple`: `i32` on wasm32, `i64` otherwise.
///
/// The one place the module header and the call sites (`FuncEmitter::size_ty`)
/// agree on it. `wasm-ld` treats a call whose type disagrees with the
/// definition as a *warning* that swaps in a trap, so a program with `malloc(i64)`
/// on wasm32 links and then dies at its first allocation; the CLI links with
/// `--fatal-warnings` so a site that forgot is a build failure that names the
/// symbol, not a trap (`docs/wasm.md`).
pub(crate) fn size_ty(triple: &Triple) -> &'static str {
    if triple.architecture == target_lexicon::Architecture::Wasm32 { "i32" } else { "i64" }
}

/// `@cancho_wasi_errno`: WASI's `errno` to the language's numbering
/// (`cancho_ir::WASI_ERRNO_TO_LINUX`, `docs/wasm.md`). A `switch` over every
/// number WASI defines; zero and anything else pass through, so "no error"
/// stays "no error".
fn wasi_errno_translation() -> String {
    let mut text = String::from("define internal i32 @cancho_wasi_errno(i32 %e) {\n");
    text.push_str("entry:\n  switch i32 %e, label %other [\n");
    for &(_, wasi, linux) in cancho_ir::WASI_ERRNO_TO_LINUX {
        if wasi != linux {
            text.push_str(&format!("    i32 {wasi}, label %to{linux}\n"));
        }
    }
    text.push_str("  ]\n");
    let mut seen = std::collections::BTreeSet::new();
    for &(_, wasi, linux) in cancho_ir::WASI_ERRNO_TO_LINUX {
        if wasi != linux && seen.insert(linux) {
            text.push_str(&format!("to{linux}:\n  ret i32 {linux}\n"));
        }
    }
    text.push_str("other:\n  ret i32 %e\n}\n\n");
    text
}

/// `@cancho_smul_overflow`: a signed 64-bit multiply that reports overflow,
/// with no libcall (`docs/wasm.md`, W0.3).
///
/// `llvm.smul.with.overflow.i64` is one `imul` and a flag on x86-64 and
/// AArch64, but on `wasm32` LLVM expands it through a 128-bit multiply, a call
/// to `__multi3` in compiler-rt's builtins, which the wasi-libc sysroot does not
/// ship, so four accept fixtures failed to link. This is the same answer
/// computed from 32-bit halves, so the module needs nothing from outside it.
///
/// On magnitudes `|a| * |b|`, as unsigned (`|INT_MIN|` is 2^63, which fits):
/// split each into high and low 32 bits. If both highs are non-zero the product
/// is at least 2^64. Otherwise one cross term is zero, the other is a single
/// 32x32 product, and it must itself fit in 32 bits to be shifted up; the low
/// product is added, and a carry out is overflow. What is left is a magnitude
/// that must fit the signed range for its sign: at most 2^63 if negative, at
/// most 2^63-1 if not. The result is that magnitude, negated if negative, and
/// is only meaningful when there was no overflow (the intrinsic says the same).
///
/// Checked against the intrinsic itself over the edge values and two million
/// random pairs by `the_inline_multiply_agrees_with_llvms_intrinsic`.
pub(crate) fn smul_overflow_definition() -> String {
    "define internal {i64, i1} @cancho_smul_overflow(i64 %a, i64 %b) {
entry:
  %an = icmp slt i64 %a, 0
  %bn = icmp slt i64 %b, 0
  %neg = xor i1 %an, %bn
  %na = sub i64 0, %a
  %nb = sub i64 0, %b
  %ua = select i1 %an, i64 %na, i64 %a
  %ub = select i1 %bn, i64 %nb, i64 %b
  %ah = lshr i64 %ua, 32
  %al = and i64 %ua, 4294967295
  %bh = lshr i64 %ub, 32
  %bl = and i64 %ub, 4294967295
  %ahz = icmp ne i64 %ah, 0
  %bhz = icmp ne i64 %bh, 0
  %both = and i1 %ahz, %bhz
  %c1 = mul i64 %ah, %bl
  %c2 = mul i64 %al, %bh
  %cross = add i64 %c1, %c2
  %crossbig = icmp ugt i64 %cross, 4294967295
  %low = mul i64 %al, %bl
  %up = shl i64 %cross, 32
  %m = add i64 %low, %up
  %carry = icmp ult i64 %m, %low
  %limit = select i1 %neg, i64 9223372036854775808, i64 9223372036854775807
  %toobig = icmp ugt i64 %m, %limit
  %o1 = or i1 %both, %crossbig
  %o2 = or i1 %o1, %carry
  %ovf = or i1 %o2, %toobig
  %nm = sub i64 0, %m
  %r = select i1 %neg, i64 %nm, i64 %m
  %p0 = insertvalue {i64, i1} undef, i64 %r, 0
  %p1 = insertvalue {i64, i1} %p0, i1 %ovf, 1
  ret {i64, i1} %p1
}

"
    .to_owned()
}
