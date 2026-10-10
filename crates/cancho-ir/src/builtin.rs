//! The functions the compiler provides rather than a program defining
//! them, and what each one's signature and effect are.

use crate::*;

/// Functions the compiler provides rather than the program defining them.
///
/// M0/M1 scaffolding: `putchar` is how a program produces output before there
/// is any FFI. M2 replaces it with a capability-gated foreign call — output is
/// an effect, and an effect must be granted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Builtin {
    /// `putchar[&i](io: &!i Io, c: int) -> [io_write] int` — libc's `putchar`,
    /// byte for byte, behind the capability that authorises it.
    ///
    /// The `Io` is not passed to libc and has no runtime representation. It
    /// is there so that a function which does not hold one cannot call this,
    /// which is the whole safety story stated as a type (§8.2).
    PutChar,
    /// `write_bytes[&i, &b](io: &!i Io, bytes: &b [byte]) -> [io_write] int` —
    /// a whole slice, behind the same capability (`docs/bulk-io.md` §3).
    ///
    /// The comment above `PutChar` said M2 would replace it with "a
    /// capability-gated foreign call". That turned out to be the wrong
    /// shape and `bulk-io.md` §2 is why: `examples/serve/` *had* the
    /// foreign call, and reaching it cost `Ffi("libc")` — which
    /// `reach.md` §5 establishes is every authority at once. So a
    /// program that wanted to print quickly had to ask for everything,
    /// and the incentive ran backwards.
    ///
    /// This is a second primitive behind the **same** capability
    /// instead. It changes what a grant of `Io` is worth without
    /// changing what it permits: the row is still `io_write`, and a
    /// faster program is not a more powerful one (§3.2).
    Write,
    /// `write_err[&i, &b](io: &!i Io, bytes: &b [byte]) -> [err_write] int`
    /// — the same slice, on the other stream (`docs/standard-error.md`).
    ///
    /// A third label under the *same* capability, which is
    /// `standard-input.md` §2's rule applied a third time: the capability
    /// is what you hold, the label is what you did with it. Standard error
    /// is part of the console — same process, same three descriptors, same
    /// shell redirecting them — so it is not an eighth field on `Split`.
    ///
    /// There is no per-byte twin. `putchar` exists beside `write_bytes`
    /// because it came first, not because two primitives were wanted, and
    /// a diagnostic is short: §3.2.
    WriteErr,
    /// `flush_out[&i](io: &!i Io) -> [io_write] Done` -- flush standard
    /// output and say whether everything written to it arrived
    /// (`docs/checked-output.md`, edition 5).
    ///
    /// `write_bytes` answers what the stdio buffer took; the last buffer's
    /// worth was written by libc at exit with the result ignored, so a
    /// program writing to a full disk could not know (`docs/bulk-io.md`
    /// §3.3, corrected). This is `fflush(stdout)` and then
    /// `ferror(stdout)`: the second because `fflush` answers 0 for an empty
    /// buffer even after an earlier write failed. Its label is `io_write`,
    /// the label of what it completes.
    FlushOut,
    /// `getchar[&i](io: &!i Io) -> [io_read] int` — libc's `getchar`, one
    /// byte in, behind the capability that authorises it.
    ///
    /// `docs/standard-input.md`. The mirror of [`Builtin::PutChar`] in
    /// every respect: the same capability, the other direction, its own
    /// effect label, and the `Io` erased on the way to libc because it
    /// carries no data.
    ///
    /// `-1` at end of input. A byte is 0..255, so the sentinel cannot be
    /// one — which is why C's `getchar` returns `int` too — and it matches
    /// `fs_read`'s `-1` for a file that could not be read. §3.1 says why
    /// an enum would be better and §6 keeps it open.
    GetChar,
    /// `split(w: World) -> [] Split` — consumes the root of all authority
    /// and hands back its parts (§8.2).
    ///
    /// The one place a capability comes from. There is no ambient
    /// constructor, no `Io::global()`, and nothing that conjures one.
    Split,
    /// `narrow(f: Ffi(a), "libc") -> [] Ffi("libc")` — attenuation (§7.4).
    ///
    /// Consumes the wider capability and hands back the narrower one, which
    /// is what makes it a trade rather than a copy: a program cannot keep
    /// the broad authority *and* the narrow one.
    ///
    /// Narrowing only, in both directions — the same commitment `lex-os`
    /// makes for manifests, for the same reason: a program must not be able
    /// to grant itself what it was not given. The literal argument is what
    /// makes the refinement checkable structurally, which is why §7.4
    /// requires one.
    Narrow,
    /// `fork_heap(h: &!x Heap) -> [heap] Heap` — a second owned `Heap`, made
    /// from a unique borrow of the first (`docs/parallelism.md` §8).
    ///
    /// Nothing is amplified: the caller already holds the authority and the
    /// label stays `heap`. The child is an ordinary `res` value, moved into a
    /// worker's struct or a spawn payload. Sound only while `Heap` has no
    /// state of its own (§8.3); it carries no leaves, so neither backend
    /// emits anything for the call. Edition 4, like `spawn`.
    ForkHeap,
    /// `copy_within(buf: &!r [byte], dst: int, src: int, n: int) -> int` — move `n` bytes inside one slice from `src` to
    /// `dst`, as `memmove` does: the ranges may overlap and the result is as if the bytes were copied out first.
    ///
    /// Bounds-checked like indexing: it traps unless `0 <= dst`, `0 <= src`, `0 <= n`, `dst + n <= len(buf)` and
    /// `src + n <= len(buf)` (the sums are not formed, so nothing can overflow). Answers 0. Pure and capability-free:
    /// it reads and writes only the slice it was given. Edition 5. `docs/memory-moves.md`.
    CopyWithin,
    /// `copy_into(dst: &!d [byte], src: &s [byte]) -> int` — copy all of `src` to the front of `dst`, as `memmove` does,
    /// and answer `len(src)`.
    ///
    /// Bounds-checked like indexing: it traps unless `len(src) <= len(dst)`. The two slices may be views of one buffer,
    /// so the copy is defined for overlap. Pure and capability-free: it touches only the slices it was given. Edition 5.
    /// `docs/bulk-copy.md`.
    CopyInto,
    /// `index_of_byte(text: &t [byte], b: byte) -> int` — where `b` first occurs in `text`, or -1 if it does not, as
    /// `memchr` answers. Pure and capability-free: it reads only the slice it was given. Edition 5.
    /// `docs/byte-search.md`.
    IndexOfByte,
    /// `fork_clock(c: &x Clock) -> Clock` — a second owned `Clock` from a
    /// shared borrow of the first (`docs/parallelism.md` §9).
    ///
    /// A thread that runs a server loop needs a clock of its own, and `split`
    /// hands out one. The capability reads the monotonic clock and nothing
    /// else, the parent already holds the authority, and the effect row has
    /// no label for it, so nothing is amplified. It does end the property
    /// that a capability has exactly one holder, for `Heap` and `Clock` only:
    /// `Net` deliberately has no fork. Edition 5, like `clock_ms`.
    ForkClock,
    /// `release(io: Io) -> [] int` — destroys a capability.
    ///
    /// Authority is a resource and a resource is destroyed exactly once, so
    /// a program that forgets this does not compile (§8.3). It is an
    /// ordinary consumer, in the sense §4.1 means.
    Release,
    /// `wrapping_add(a: int, b: int) -> [] int`, and its two siblings —
    /// two's-complement arithmetic that wraps instead of trapping.
    ///
    /// `+` is checked (`docs/defined-behaviour.md`), because a silently
    /// wrong answer is what the whole design refuses. But wraparound is the
    /// *intent* in a checksum, a hash or a counter, and a language that
    /// cannot express it forces the workaround to be worse than the thing.
    /// So it is spelled out: wrapping is what you asked for, not what you
    /// got away with.
    WrappingAdd,
    WrappingSub,
    WrappingMul,
    /// `value_barrier(x: int) -> [] int` -- `x`, which the optimiser may
    /// assume nothing about (`docs/value-barrier.md`, edition 6 only).
    ///
    /// Constant-time code selects under masks, and `clang -O2` can prove
    /// a mask built from a sign bit is 0 or -1 and turn the `and` back
    /// into a branch on the secret. A mask passed through this is just a
    /// number to it. On LLVM it is an empty `asm` tying output to input;
    /// on Cranelift, which makes no such branches, the identity.
    ValueBarrier,
    /// `hw_aes_gcm() -> [] bool` — whether this CPU has the instructions
    /// `aes_encrypt_block` and `ghash_update` use (`docs/crypto-builtins.md`
    /// §3): AES, PCLMULQDQ and SSSE3 on x86-64, AES and PMULL on aarch64.
    /// The CPU is read once. False on Cranelift and WebAssembly, which have
    /// no such instructions (§5).
    HwAesGcm,
    /// `aes_encrypt_block(round_keys: &[byte], rounds: int, block: &[byte],
    /// out: &![byte]) -> [] int` — the FIPS-197 cipher on one block with the
    /// CPU's AES instructions (§3). `round_keys` is the expanded key,
    /// `rounds + 1` blocks of 16 bytes; `rounds` is 10, 12 or 14. A wrong
    /// length or round count traps, as an index out of bounds does: the
    /// lengths are the caller's, never the network's. Answers 0. Only
    /// where `hw_aes_gcm()` is true; elsewhere it traps.
    AesEncryptBlock,
    /// `ghash_update(h: &[byte], y: &![byte], data: &[byte]) -> [] int` —
    /// GCM's GHASH (SP 800-38D §6.4) over every 16-byte block of `data`:
    /// `y = (y XOR block) * h` in GF(2^128), in GCM's bit order, `y`
    /// updated in place (§3). `h` and `y` are 16 bytes and `data` a
    /// multiple of 16, or it traps. Answers 0. Only where `hw_aes_gcm()` is
    /// true; elsewhere it traps.
    GhashUpdate,
    /// `byte_of(n: int) -> [] byte` — narrow an integer to a byte, or trap.
    ///
    /// `docs/strings.md` §2: it traps outside 0..255 rather than
    /// truncating, because truncation is the silently wrong answer
    /// `defined-behaviour.md` §2.1 already refused for `+`. A caller that
    /// wants the low eight bits says so, once there is a mask to say it
    /// with.
    ByteOf,
    /// `float_of(n: int) -> [] float` — the nearest `float` to an `int`
    /// (`docs/floating-point.md` §4).
    ///
    /// Rounds rather than trapping, which is the deliberate difference
    /// from `byte_of`: `byte_of(300)` loses the magnitude and hands back
    /// a different number, where this loses at most one unit in the last
    /// place, under IEEE's round-to-nearest-even. One is a lie about
    /// which number this is; the other is what a floating type *means*.
    FloatOf,
    /// `truncate(x: float) -> [] int` — toward zero, trapping on NaN,
    /// ±infinity and any magnitude at or past `2^63` (§4).
    ///
    /// Exactly the inputs C leaves undefined. The name states the
    /// rounding because the rounding is what a reader needs to know;
    /// `floor`, `ceil` and round-to-nearest belong in `std.math`, where
    /// each can say which it is.
    Truncate,
    /// `bits_of(x: float) -> [] int` — the IEEE-754 representation, read
    /// as an integer (`docs/float-printing.md` §2).
    ///
    /// A *reinterpretation*, not a conversion: the bits are unchanged and
    /// IEEE-754 says exactly what they mean. `float_of` and `truncate`
    /// are the conversions, and both are about values.
    ///
    /// It exists so numeric library code can be written **in the
    /// language** rather than in the compiler. Without it, decomposing a
    /// float into sign, exponent and mantissa is impossible, and every
    /// routine that needs to — printing, `copysign`, `frexp`, a total
    /// order — has to become a builtin. `std.fmt` is the first caller and
    /// is the argument: a correct shortest-round-trip printer, written in
    /// cancho, rather than a hole in the standard library.
    BitsOf,
    /// `float_of_bits(n: int) -> [] float` — the `float` whose 64 bits are
    /// `n`: `bits_of`'s inverse, and as little a conversion as it is
    /// (`docs/floating-point.md` §4.1). Pure and capability-free. Edition 7,
    /// because a program may already declare the name.
    ///
    /// Every pattern is a value, a NaN's payload included; `bits_of` still
    /// answers one pattern for every NaN, so the pair is an identity on
    /// every non-NaN and on none of the NaN payloads.
    FloatOfBits,
    /// `f32_of(x: float) -> [] f32` — the nearest `f32` to a `float`,
    /// ties to even, an overflow giving infinity (`docs/f32.md` §2).
    ///
    /// A rounding, and named for being one: it is the one crossing from
    /// `float` to `f32`, written where a reader can see it. Never traps.
    F32Of,
    /// `float_of32(x: f32) -> [] float` — exact, every binary32 value is
    /// a binary64 value (`docs/f32.md` §2).
    ///
    /// Not `float_of`: that name is `int -> float` and a builtin has one
    /// signature, so the width is part of the name, as in `bits_of32`.
    FloatOf32,
    /// `bits_of32(x: f32) -> [] int` — the 32 bits of binary32, zero
    /// extended (`docs/f32.md` §2). A reinterpretation like `bits_of`,
    /// and like it every NaN answers one pattern, `0x7fc00000`, so the
    /// answer does not depend on which target generated the NaN.
    BitsOf32,
    /// `f32_of_bits(n: int) -> [] f32` — the `f32` whose bits are the low
    /// 32 of `n` (`docs/f32.md` §2). The inverse `float-printing.md` §8
    /// left for when something needs it; `cancho-gpu` and the gate in
    /// `docs/f32.md` §5 both do.
    F32OfBits,
    /// `sqrt32(x: f32) -> [] f32` -- the correctly rounded square root, one
    /// instruction (`docs/f32.md` §2, `docs/float-math.md` §3). Named as
    /// `bits_of32` is: the width is part of the name because `sqrt` is
    /// `float -> float` and a builtin has one signature.
    Sqrt32,
    /// `f32_of_int(n: int) -> [] f32` -- the nearest `f32`, ties to even;
    /// every `int` is in range, so nothing traps (`docs/f32.md` §2).
    F32OfInt,
    /// `int_of_f32(x: f32) -> [] int` -- toward zero, trapping on NaN,
    /// infinity and any magnitude at or beyond `2^63`: `truncate`'s rule
    /// at the narrower width (`docs/floating-point.md` §4).
    IntOfF32,
    /// `is_nan(x: float) -> [] bool` (§5).
    ///
    /// Exists because NaN breaks comparison — `x == x` is false for it —
    /// so the hazard has to be checkable, and `x != x` is a riddle
    /// rather than a test.
    IsNan,
    /// `sqrt(x: float) -> [] float` — the square root, correctly rounded.
    ///
    /// A builtin rather than library code, which is the opposite of the
    /// call `float-printing.md` made for printing, and the reason is
    /// measured rather than assumed (`docs/float-math.md` §2): **a
    /// correctly-rounded square root cannot be written in cancho.** The
    /// two programs that hand-rolled one got 58.4% of values wrong in
    /// the last place and one of them was wrong by 143 orders of
    /// magnitude on a large input. IEEE-754 requires `sqrt` to be
    /// correctly rounded and the hardware instruction is, so the
    /// instruction is the only correct implementation available.
    ///
    /// Not a libc call, which is the whole of the capability question
    /// (§3): `sqrtsd` and `fsqrt` are one instruction each, so this
    /// reaches no library, needs no `Ffi`, and its row is `[]`.
    Sqrt,
    /// `int_of(b: byte) -> [] int` — widen a byte, which is always defined
    /// and always lands in 0..255.
    IntOf,
    /// `fs_read(fs, path, into) -> [fs_read(p)] int` — read a whole file.
    ///
    /// `docs/filesystem.md` §2: a builtin rather than an `extern fn`,
    /// because an `extern` would be gated by `Ffi("libc")` and then holding
    /// the *FFI* capability would open any path, with `Fs` contributing
    /// nothing. The authority that guards the filesystem has to be the one
    /// that names it, so the backend reaches libc itself, the way `putchar`
    /// and the arena already do.
    ///
    /// Returns the byte count, or `-1` if the file could not be read: a
    /// missing file is an ordinary outcome, not a broken promise. A path
    /// *outside* the capability's prefix is the broken promise, and traps.
    FsRead,
    /// `open_read[&c, &a](fs: &c Fs(p), path: &a [byte]) -> [fs_read(p)] Opened`
    /// — `docs/file-handles.md` §2.1.
    ///
    /// Checked at the call site like [`Builtin::FsRead`] and for the same
    /// reason: the prefix lives in the capability's type, and a fixed
    /// signature has no parameter to name it. The whole path check is paid
    /// here, once, which is §4's fourth answer — the handle it returns
    /// cannot be widened, so `read` performs a path-free label.
    OpenRead,
    /// `file_read[&f, &b](file: &!f File, into: &!b [byte]) -> [file_read] Read`
    /// — §3.
    ///
    /// Named the way `fs_read` is — the subject, then the verb — and
    /// sharing its name with the label it performs, exactly as `fs_read`
    /// does. The design doc wrote it `read`, and `read` turned out to be
    /// a name a program wants: `examples/serve/` declares `extern fn read`
    /// for libc's, on a socket rather than a file.
    ///
    /// Three outcomes and three constructors. A sentinel is how `getchar`
    /// and `fs_read` came to disagree about `-1`, so this is the API that
    /// does not repeat it.
    ReadFile,
    /// `file_close(file: File) -> [] int` — §2. Renamed from `close` for
    /// the reason above: 31 fixtures had a `close` of their own.
    ///
    /// Consumes the handle, which is what `res` means; the checker needed
    /// nothing new to enforce it. The `int` is the outcome of `close(2)`,
    /// which can fail even though nothing can be done about it.
    Close,
    /// `open_append`, `open_write`, `open_new` and `open_rw`
    /// (`docs/file-writes.md` section 3): `open_read`'s shape, one for each
    /// way a log opens a file. Checked at the call site for the reason
    /// `open_read` is: the prefix lives in the capability's type.
    OpenAppend,
    OpenWrite,
    OpenNew,
    OpenRw,
    /// `fs_rename(fs, from, to)` and `fs_remove(fs, path)`, both `[fs_write(p)] Done`
    /// (`docs/file-writes.md` section 7): checked at the call site, because
    /// the prefix is in the capability's type and every path is checked against it.
    FsRename,
    FsRemove,
    /// `file_lock(&!File) -> [file_write] Done`: `flock(LOCK_EX | LOCK_NB)`, an advisory
    /// lock the kernel releases when the process ends, however it ends.
    FileLock,
    /// `file_write(&!File, &[byte]) -> [file_write] Done`: one `write(2)`,
    /// which may take fewer bytes than asked (section 4.1).
    FileWrite,
    /// `file_pwrite(&!File, at, &[byte]) -> [file_write] Done`: `pwrite(2)`.
    /// On a handle opened for append it still appends (section 4.2).
    FilePwrite,
    /// `file_pread(&!File, at, &![byte]) -> [file_read] Read`: `pread(2)`.
    FilePread,
    /// `file_sync(&!File) -> [file_write] Done`: `fsync(2)` (section 5).
    FileSync,
    /// `file_truncate(&!File, len) -> [file_write] Done`: `ftruncate(2)`.
    FileTruncate,
    /// `file_size(&!File) -> [file_read] Done`: the length, with the cursor
    /// left where it was (section 4.3).
    FileSize,
    /// `fs_write(fs, path, bytes) -> [fs_write(p)] int` — write a whole file.
    FsWrite,
    /// `box(h, value) -> [heap] Box[T]` — one value, one allocation.
    ///
    /// `docs/heap.md` §3. A builtin rather than an `extern fn` for the same
    /// reason the file operations are (`filesystem.md` §2): an `extern`
    /// would be gated by `Ffi("libc")`, and then the FFI capability would
    /// allocate, with `Heap` contributing nothing.
    ///
    /// Checked at the call site, because the result's type is the
    /// argument's and a fixed signature has no parameter to bind it to.
    Box,
    /// `unbox(h, b: Box[T]) -> [heap] T` — free the allocation, yield the value.
    ///
    /// The only consumer a `Box` has. That is what makes §3.1's claim hold:
    /// a box is `res`, so a program that never unboxes one does not compile,
    /// and the general heap cannot leak.
    Unbox,
    /// `contents(b: &r Box[T]) -> [] &r T` — the dereference.
    ///
    /// Mode- and region-preserving: a shared borrow of a box yields a shared
    /// borrow of what it holds, for exactly as long. There is nothing to
    /// check at run time, because there is no way to hold a reference into a
    /// box that has been freed -- `unbox` consumes, and §5 already refuses a
    /// reference that outlives its borrow.
    Contents,
    /// `box_slice(h, count, fill) -> [heap] Box[[T]]` — a run of values on
    /// the heap (`docs/boxed-slices.md` §3).
    ///
    /// The second shape a box comes in: a pointer *and* a length, where an
    /// ordinary box is a pointer alone, because nothing else knows how many
    /// elements there are.
    BoxSlice,
    /// `unbox_slice(h, b) -> [heap] int` — free it, and answer how many.
    ///
    /// A different operation from `unbox`, and it has to be: `unbox` hands
    /// back what the box held, and `[T]` is unsized so there is nothing to
    /// hand back. It is still the *only* consumer a boxed slice has, so
    /// `heap.md` §3.1 holds unchanged.
    UnboxSlice,
    /// `arg_count(a) -> [args] int` — `argc`, exactly as the runtime gave it.
    ///
    /// `docs/arguments.md` §3. A builtin rather than an `extern fn` for the
    /// reason `filesystem.md` §2 gives: an `extern` would be gated by
    /// `Ffi("libc")`, and then the FFI capability would read the command
    /// line with `Args` contributing nothing.
    ArgCount,
    /// `arg(a, n) -> [args] &static [byte]` — one argument, as bytes.
    ///
    /// `arg(a, 0)` is the program name. The region is `static` because
    /// argv outlives every region in the program (§3.1), and the slice is
    /// *shared* because a program does not own its own command line.
    Arg,
    /// `len(s: &r [T]) -> [] int` — how many elements a slice has.
    ///
    /// Checked at the call site rather than through a written signature,
    /// because the element type is whatever the argument's is and a fixed
    /// signature cannot say that without a type parameter the builtin
    /// table has no way to bind.
    Len,
    /// `connect(net, name, port) -> [net_out(bound)] int` —
    /// `docs/net.md` §4.1, edition 2 only (`docs/editions.md` §7).
    ///
    /// The name is checked against the capability's bound, then resolved
    /// with `getaddrinfo` (`docs/connect.md` §10). A socket operation
    /// rather than an `extern fn`, for the reason `filesystem.md` §2 gives
    /// for `fs_read`: an `extern` would be gated by `Ffi("libc")` alone,
    /// and then the FFI capability would open any socket, with `Net`
    /// contributing nothing.
    ///
    /// Checked at the call site like [`Builtin::FsRead`], because the row
    /// it performs is the bound its `Net` capability was narrowed to, and
    /// a fixed signature has nowhere to put it.
    Connect,
    /// `bind(net, port) -> [net_in(bound)] int` — `docs/net.md` §2.1,
    /// edition 2 only (`docs/listen.md` §6).
    ///
    /// The inbound mirror of [`Builtin::Connect`]: folds `socket`,
    /// `setsockopt(SO_REUSEADDR)` and `bind` into one call, and checks
    /// `port` against the capability's bound -- here just a port, not a
    /// `host:port` pair, because `net.md` §2.1 bounds inbound by *which
    /// port* alone (`docs/listen.md` §6.1). Checked at the call site for
    /// the same reason `connect` is.
    Bind,
    /// `listen(fd, backlog) -> [] int` — `listen(2)`, unchanged.
    ///
    /// Takes no capability: the fd already proves the authority `bind`
    /// checked, the same way `read`/`close` need none once `open_read`
    /// has run (`docs/file-handles.md` §4.1). A fixed signature, unlike
    /// `bind` and `connect`, because nothing about it depends on a
    /// literal written at the call (`docs/listen.md` §6).
    Listen,
    /// `accept(fd) -> [] int` — `accept(2)`, the peer address ignored
    /// (`NULL, NULL`, as `examples/serve/`'s own hand-written call
    /// already does). Fixed, for the same reason `listen` is.
    Accept,
    /// `tcp_listen(net, port, backlog, flags) -> [net_in(bound)] Listening`
    /// -- `docs/native-sockets.md` §3, edition 5 only.
    ///
    /// `socket`, `SO_REUSEADDR`, `bind` and `listen` in one call, answering
    /// a `Listener` handle (or the `errno`) rather than a descriptor a
    /// program could forge. `flags` bit 1 is `SO_REUSEPORT`. Checked at the
    /// call site, like [`Builtin::Bind`], because the row it performs is
    /// the bound its `Net` was narrowed to.
    TcpListen,
    /// `tcp_connect(net, host, port) -> [net_out(bound)] Dialed` --
    /// `docs/native-sockets.md` §3, edition 5 only: `connect`, answering a
    /// `Conn` handle (or the reason) rather than a descriptor.
    TcpConnect,
    /// `tcp_connect_start(net, host, port) -> [net_out(bound)] Dialed` --
    /// `docs/native-sockets.md` §10.6, edition 5 only: `tcp_connect` that does
    /// not wait. The `Conn` is non-blocking and the connection may still be in
    /// progress: watch it for *writable* with a `Poller`, then ask
    /// `conn_connect_status`. A name is still resolved by a blocking call; an IP
    /// literal needs none.
    TcpConnectStart,
    /// `poller_new() -> [] Polling` -- `docs/native-sockets.md` §4: an
    /// `epoll` (Linux) or `kqueue` (Darwin) set, empty. It needs no
    /// capability: a set watching nothing observes nothing.
    PollerNew,
    /// `poller_add_listener(&!Poller, &Listener, token) -> [poll] int`:
    /// watch a listener for connections. `0`, or the `errno`.
    PollerAddListener,
    /// `poller_add_conn(&!Poller, &Conn, token, events) -> [poll] int`:
    /// `events` is 1 for readable, 2 for writable, 3 for both.
    PollerAddConn,
    /// `poller_modify(&!Poller, &Conn, token, events) -> [poll] int`.
    PollerModify,
    /// `poller_remove(&!Poller, &Conn) -> [poll] int`.
    PollerRemove,
    /// `poller_wait(&!Poller, &![int], timeout_ms) -> [poll] int`: writes
    /// `(token, events)` pairs into the slice (at most 64 at a time) and
    /// answers how many, or `-errno`. A negative timeout waits for ever.
    PollerWait,
    /// `poller_close(Poller) -> [] int`.
    PollerClose,
    /// `clock_ms(&Clock) -> [clock] int` -- `docs/native-sockets.md` §5,
    /// edition 5 only: monotonic milliseconds from an arbitrary origin, so
    /// an idle timeout survives the wall clock being set.
    ClockMs,
    /// `clock_unix_ms(&Clock) -> [clock] int` -- `docs/native-sockets.md`
    /// §10.5, edition 5 only: milliseconds since 1970-01-01 UTC, the
    /// wall clock. It can jump backwards or forwards when the host's clock
    /// is set, so it is for stamping messages and never for timeouts.
    ClockUnixMs,
    /// `conn_detach(Conn) -> [] int` -- `docs/native-sockets.md` §10.3:
    /// turns a connection into an inert **ticket** (an integer) so it can
    /// sit in a `Vec`, which holds only copyable things. The `Conn` is
    /// consumed; the descriptor stays open. `-1` if it could not be done,
    /// in which case the connection has been closed.
    ConnDetach,
    /// `conn_attach(int) -> [] Attached` -- redeems a ticket **once**. A
    /// ticket that was never issued, was already redeemed, or belongs to a
    /// descriptor since reused is `Failed(EBADF)`: the number is not
    /// authority, and forging one reaches nothing.
    ConnAttach,
    /// `tcp_accept(&!Listener) -> [conn_accept] Accepted`.
    TcpAccept,
    /// `conn_read(&!Conn, &![byte]) -> [conn_read] Received`.
    ConnRead,
    /// `conn_write(&!Conn, &[byte]) -> [conn_write] Sent`: never waits on a
    /// non-blocking connection, and never raises `SIGPIPE`.
    ConnWrite,
    /// `conn_nonblocking(&!Conn) -> [] int`: one way, explicit.
    ConnNonblocking,
    /// `conn_nodelay(&!Conn) -> [] int`: `TCP_NODELAY` on, one way, explicit. `0`, or
    /// the `errno`. A program that writes a message in more than one piece, or answers
    /// a request in several writes, waits on the other end's delayed acknowledgement
    /// (tens of milliseconds) without it (`docs/native-sockets.md` section 11).
    ConnNodelay,
    /// `conn_peer(&Conn, &![byte]) -> [] int` -- `docs/conn-peer.md`: the address and port of the
    /// other end of a connection, written into the caller's buffer as 19 bytes (family `4` or `6`,
    /// sixteen address bytes, the port big-endian), by `getpeername(2)`. `0`, or the `errno`
    /// (`EINVAL` for a buffer under 19 bytes or a socket that is not IP). It names no resource and
    /// grants nothing: the address is data the program receives, not authority to dial it.
    ConnPeer,
    /// `conn_connect_status(&!Conn) -> [] int` -- `docs/native-sockets.md` §10.6:
    /// how a connection started with `tcp_connect_start` ended: `0` connected, or
    /// the `errno` (`SO_ERROR`). Meaningful only once a `Poller` has reported the
    /// connection writable (or hung up); before that it answers `0` whether or not
    /// the connection is made.
    ConnConnectStatus,
    /// `listener_nonblocking(&!Listener) -> [] int`.
    ListenerNonblocking,
    /// `conn_close(Conn) -> [] int`: consumes the handle.
    ConnClose,
    /// `listener_close(Listener) -> [] int`.
    ListenerClose,
    /// `udp_connect(net, host, port) -> [net_out(bound)] UdpOpened` --
    /// `docs/udp.md` §2, edition 5 only: `tcp_connect`'s check and walk on a
    /// `SOCK_DGRAM` socket, then `connect(2)`, so the kernel itself sends only to that
    /// peer and drops datagrams from any other source.
    UdpConnect,
    /// `udp_recv(&!Udp, &![byte]) -> [udp_recv] Datagram`: one datagram, never a
    /// prefix of one without saying so (`Truncated`).
    UdpRecv,
    /// `udp_send(&!Udp, &[byte]) -> [udp_send] Sent`: the whole datagram or nothing.
    UdpSend,
    /// `udp_local_port(&Udp) -> [] int`: the port the kernel chose, or `-errno`.
    UdpLocalPort,
    /// `udp_peer(&Udp, ticket, &![byte]) -> [] int` -- `docs/udp.md` §12: who a ticket names, as data. Writes the sender's
    /// address and port into the caller's buffer in the form `conn_peer` uses (19 bytes: family, sixteen address bytes, the port
    /// big-endian; a datagram's sender is always family `4` until sockets are dual-stack), after the three checks `udp_send_to`
    /// makes. `0`, or `EBADF` for a ticket not valid on this socket, or `EINVAL` for a buffer under 19 bytes (nothing is written
    /// then). It reaches nothing: there is still no send to an address.
    UdpPeer,
    /// `udp_nonblocking(&!Udp) -> [] int`: one way, explicit.
    UdpNonblocking,
    /// `udp_close(Udp) -> [] int`: consumes the handle.
    UdpClose,
    /// `udp_detach(Udp) -> [] int` -- `docs/udp.md` §11: `conn_detach` for a datagram socket. The `Udp`
    /// ends, the descriptor stays open, and what comes back is a **ticket** a `Vec[int]` can hold. The
    /// ticket carries a kind bit, so a `Conn`'s ticket does not redeem as a `Udp` nor the reverse. `-1`
    /// if it could not be done, in which case the socket has been closed.
    UdpDetach,
    /// `udp_attach(int) -> [] UdpOpened` -- redeems a `udp_detach` ticket **once**. A ticket never
    /// issued, already redeemed, of the other kind, or for a descriptor since reused is `Failed(EBADF)`.
    UdpAttach,
    /// `udp_bind(net, port, flags) -> [net_in(bound)] UdpOpened` -- `docs/udp.md` §2, edition 5
    /// only: `tcp_listen` for datagrams (`socket`, `SO_REUSEADDR`, `bind`; no `listen`). Checked
    /// at the call site, like [`Builtin::TcpListen`]. `flags` bit 1 is `SO_REUSEPORT`.
    UdpBind,
    /// `udp_recv_from(&!Udp, &![byte], &![int]) -> [udp_recv] Datagram` -- `docs/udp.md` §4:
    /// `udp_recv` that also records the sender in the runtime's peer ring and writes a **ticket**
    /// for it into the first cell of the `int` slice. A ticket is the only way to name a
    /// destination for [`Builtin::UdpSendTo`]; an empty slice is `Failed(EINVAL)`.
    UdpRecvFrom,
    /// `udp_send_to(&!Udp, &[byte], int) -> [udp_send] Sent` -- `docs/udp.md` §4: send one datagram
    /// to the sender a ticket names. A ticket that was never issued, was issued to another socket,
    /// or is older than the ring (65,536 datagrams) is `Failed(EBADF)`.
    UdpSendTo,
    /// `poller_add_udp(&!Poller, &Udp, token, events) -> [poll] int`: `events` as `poller_add_conn`'s.
    /// There is no modify or remove: closing the socket removes it (`docs/udp.md` §3).
    PollerAddUdp,
    /// `tty_open(t: &Tty(bound), path: &[byte]) -> [tty_open(bound)] TtyOpened` --
    /// `docs/tty.md` §3, edition 8 only: open a serial port at `path`,
    /// under the capability's prefix. Checked at the call site, like
    /// [`Builtin::UdpBind`]: the path is a run-time value, the bound is
    /// in the capability's type. `O_RDWR | O_NOCTTY | O_NONBLOCK`.
    TtyOpen,
    /// `tty_configure(&!Port, baud: int) -> [] int` -- raw mode, 8N1, and
    /// the speed, one call and no mode algebra (`docs/tty.md` §3). `0`, or
    /// the `errno` the platform answered (`EINVAL` on macOS for a speed
    /// past its standard set, measured by the spike).
    TtyConfigure,
    /// `tty_read(&!Port, into: &![byte]) -> [] int` -- what is there,
    /// never blocks (`O_NONBLOCK`; the poller is the waiting story).
    /// `-1` on error, the `Conn` verbs' convention.
    TtyRead,
    /// `tty_write(&Port, bytes: &[byte]) -> [] int` -- whole, or short.
    /// `-1` on error.
    TtyWrite,
    /// `tty_flush_input(&!Port) -> [] int` -- `tcflush(TCIFLUSH)`.
    TtyFlushInput,
    /// `tty_close(Port) -> [] int` -- consumes the handle.
    TtyClose,
    /// `poller_add_tty(&!Poller, &Port, token, events) -> [poll] int`:
    /// `events` as `poller_add_conn`'s, the family's sixth member.
    PollerAddTty,
    /// `signals_watch(&Signals("S")) -> [signals("S")] Watching` --
    /// `docs/signals.md` section 2, edition 6 only: claim the signals `S` the
    /// capability was narrowed to. Checked at the call site (`Expr::Call`
    /// with the set's bits as a second argument) because its row is the set
    /// in the capability's type, as `tcp_listen`'s is the bound's.
    SignalsWatch,
    /// `signals_pending(&!SignalWatch) -> [signals_read] int`: the bits of the
    /// claimed signals that arrived since the previous call, cleared. Never
    /// waits.
    SignalsPending,
    /// `poller_add_signals(&!Poller, &SignalWatch, token) -> [poll] int`:
    /// watch a claim for readability. `0`, or the `errno`.
    PollerAddSignals,
    /// `signals_close(SignalWatch) -> [] int`: ends the claim and puts the
    /// signals back to the default; consumes the handle.
    SignalsClose,
    /// `open_dir(&Fs(p), path) -> [fs_read(p)] DirOpened`: a directory handle,
    /// the anchor everything after it is opened beneath. Lowered as
    /// `Expr::OpenFile` with `OpenMode::Directory`, so the prefix check is
    /// `open_read`'s. Edition 6. `docs/directory-handles.md`.
    OpenDir,
    /// `dir_enter(&Dir, name) -> [dir_read] DirOpened`: one child directory,
    /// `openat(dir, name, O_DIRECTORY | O_NOFOLLOW)`. `name` is one component:
    /// empty, `.`, `..`, a `/` or a NUL is `Failed(EINVAL)` with no call.
    DirEnter,
    /// `dir_open_read(&Dir, name) -> [dir_read] Opened`: one child file for
    /// reading, `openat(dir, name, O_NOFOLLOW)`, with `dir_enter`'s check.
    DirOpenRead,
    /// `dir_close(Dir) -> [] int`: `close`'s answer; consumes the handle.
    DirClose,
    /// `dir_open_new(&Dir, name) -> [dir_write] Opened`: create one child file
    /// that must not exist, `openat(O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW,
    /// 0644)`. `dir_enter`'s check on `name`. `docs/directory-handles.md` §3.
    DirOpenNew,
    /// `dir_open_append(&Dir, name) -> [dir_write] Opened`: one child file
    /// for appending, created if missing, `openat(O_WRONLY | O_CREAT |
    /// O_APPEND | O_NOFOLLOW, 0644)`.
    DirOpenAppend,
    /// `dir_rename(&Dir, from, to) -> [dir_write] Done`: `renameat` with both
    /// names in the one directory; each name has `dir_enter`'s check.
    DirRename,
    /// `dir_rename_new(&Dir, from, to) -> [dir_write] Done`: `dir_rename`
    /// that never replaces a name -- `renameat2(RENAME_NOREPLACE)` on Linux,
    /// `renameatx_np(RENAME_EXCL)` on Darwin -- so an existing `to` is
    /// `Failed(EEXIST)` and its bytes are untouched. A filesystem that cannot
    /// do it is `Failed(ENOTSUP)`, never a replacing rename
    /// (`docs/directory-handles.md` §3, slice 4). Edition 7.
    DirRenameNew,
    /// `dir_remove(&Dir, name) -> [dir_write] Done`: `unlinkat(dir, name, 0)`.
    /// A link is removed, never followed.
    DirRemove,
    /// `dir_sync(&Dir) -> [dir_write] Done`: `fsync` on the directory itself,
    /// so a rename in it is durable.
    DirSync,
    /// `dir_list(&Dir) -> [dir_read] Listing`: a stream of the directory's
    /// names, on a descriptor of its own (`docs/directory-listing.md` §3.1).
    DirList,
    /// `dir_next(&!DirList, &![byte]) -> [dir_read] Listed`: the next name,
    /// copied into the buffer, with its kind; `.` and `..` never.
    DirNext,
    /// `dir_list_close(DirList) -> [] int`: `closedir`'s answer; consumes
    /// the listing.
    DirListClose,
    /// `dir_stat(&Dir, name) -> [dir_read] DirStat`: `fstatat` with
    /// `AT_SYMLINK_NOFOLLOW` on one checked component -- a link is reported,
    /// never followed (`docs/directory-listing.md` §3.2).
    DirStat,
    /// `dir_mode(&Dir, name) -> [dir_read] Done`: the permission bits
    /// (`st_mode & 0o7777`) of one checked component, by `fstatat` with
    /// `AT_SYMLINK_NOFOLLOW` as `dir_stat` (`docs/directory-listing.md`
    /// §3.5), edition 7.
    DirMode,
    /// `dir_own_mode(&Dir) -> [dir_read] Done`: the permission bits of the
    /// opened directory itself, by `fstat` on its descriptor (§3.5),
    /// edition 7.
    DirOwnMode,
    /// `pipe_open() -> [] Piped` -- `docs/processes.md` §3.2, edition 7: a
    /// channel's two ends, the parent's and the one a child is handed. An
    /// unnamed channel inside this process reaches nothing, so no capability.
    PipeOpen,
    /// `exec_spawn(&Exec(p), path, args, env, stdin, stdout, stderr) ->
    /// [exec(p)] Spawned`: start the program at `path` under `p`. Lowered as
    /// `Expr::ExecSpawn`, so the prefix travels with it, as `open_read`'s does.
    ExecSpawn,
    /// `exec_spawn_in(&Exec(p), &Dir, path, args, env, stdin, stdout, stderr)
    /// -> [exec(p)] Spawned`: `exec_spawn` whose child starts in the directory
    /// the `Dir` holds (`docs/processes.md` §4.10). Lowered as
    /// `Expr::ExecSpawn { in_dir: true, .. }`.
    ExecSpawnIn,
    /// `child_wait(Child) -> [] Exited`: wait for the child to end and reap it;
    /// the only consumer of a `Child` (§4.7).
    ChildWait,
    /// `child_kill(&Child, signal) -> [child_signal] int`: one of
    /// `std.signals`' bits, or `KILL` (256). `0`, or the `errno`.
    ChildKill,
    /// `pipe_read(&!Pipe, &![byte]) -> [pipe_read] Received`.
    PipeRead,
    /// `pipe_write(&!Pipe, &[byte]) -> [pipe_write] Sent`: never raises `SIGPIPE`.
    PipeWrite,
    /// `pipe_nonblocking(&!Pipe) -> [] int`: one way, explicit.
    PipeNonblocking,
    /// `pipe_close(Pipe) -> [] int`: consumes the parent's end.
    PipeClose,
    /// `child_end_close(ChildEnd) -> [] int`: consumes a child's end that was
    /// never handed to a child.
    ChildEndClose,
    /// `poller_add_pipe(&!Poller, &Pipe, token, events) -> [poll] int` --
    /// `docs/processes.md` §4.8, edition 7: watch a channel's parent end as
    /// `poller_add_conn` watches a `Conn`. `0`, or the `errno`.
    PollerAddPipe,
    /// `poller_add_child(&!Poller, &Child, token) -> [poll] int` -- §4.8,
    /// edition 7: report the child's exit as readable. After `poller_wait`
    /// names the token, `child_wait` answers without blocking. `0`, or the
    /// `errno` -- `ENOSYS` where the kernel gave the child no `pidfd`, `EMFILE`
    /// where the program had no descriptor to spare for it.
    PollerAddChild,
    /// `null_ptr() -> [] c_ptr` — the one producer of a `c_ptr` that is
    /// not a foreign call's return, edition 3 only
    /// (`docs/opaque-pointers.md` §3).
    ///
    /// Needed because OpenSSL's own error convention is "returns `NULL`
    /// on failure" for `SSL_CTX_new`/`SSL_new`, and the no-coercion rule
    /// on `c_ptr` forbids building that comparison value out of an `int`
    /// literal. Fixed, like `sqrt`: nothing about it depends on the
    /// call site.
    NullPtr,
    /// `spawn(payload: T, body: fn(T) -> [row] R) -> [conc] res
    /// Thread[T, R]` — a real OS thread, edition 4 only
    /// (`docs/threads.md` §2).
    ///
    /// Checked at the call site, like [`Builtin::Len`]: `T` and `R` are
    /// read off `payload`'s and `body`'s own types, not fixed by a
    /// signature, and this first slice restricts both to exactly one
    /// pointer-width leaf (`int`, `bool`, `c_ptr`, or a reference) --
    /// what `pthread_create`'s own `void *(*)(void *)` start routine
    /// can carry without a compiler-synthesised trampoline function,
    /// which nothing in this IR can build yet. `body`'s own compiled
    /// entry point becomes the start routine directly.
    Spawn,
    /// `join(handle: res Thread[T, R]) -> [row] R` — blocks until the
    /// thread `spawn` started returns, edition 4 only.
    ///
    /// Checked at the call site: `R` is read off `handle`'s own type.
    /// The only operation that consumes a `Thread[T, R]`, the same
    /// "one consumer" shape `unbox`/`close` already have.
    Join,
    /// `trap() -> [] int` — end the process now, the same way every
    /// checked operation already does (`docs/defined-behaviour.md` §1).
    ///
    /// `docs/testing.md` §2 is why: a test framework needs a primitive
    /// a library can build `assert` out of, and this repository's own
    /// answer to "what happens when an assumption fails" has been a
    /// trap since M0 -- no message, no exception, `SIGILL` on both
    /// targets, exactly like an overflowing `+` or an out-of-range
    /// index. Both backends already carry the one instruction this
    /// needs (`trapnz`/`trap_if`); this is the first builtin that
    /// reaches it **unconditionally** rather than behind an
    /// arithmetic or bounds check the compiler emits on its own.
    /// Fixed, like `sqrt`: nothing about it depends on the call site,
    /// and it needs no capability, because deciding to trap is not an
    /// effect on the world.
    Trap,
}

impl Builtin {
    pub const ALL: &'static [Builtin] = &[
        Builtin::PutChar,
        Builtin::Write,
        Builtin::WriteErr,
        Builtin::FlushOut,
        Builtin::GetChar,
        Builtin::Split,
        Builtin::Release,
        Builtin::Narrow,
        Builtin::ForkHeap,
        Builtin::ForkClock,
        Builtin::CopyWithin,
        Builtin::CopyInto,
        Builtin::IndexOfByte,
        Builtin::WrappingAdd,
        Builtin::WrappingSub,
        Builtin::WrappingMul,
        Builtin::ValueBarrier,
        Builtin::HwAesGcm,
        Builtin::AesEncryptBlock,
        Builtin::GhashUpdate,
        Builtin::Len,
        Builtin::ByteOf,
        Builtin::IntOf,
        Builtin::FloatOf,
        Builtin::Truncate,
        Builtin::IsNan,
        Builtin::Sqrt,
        Builtin::BitsOf,
        Builtin::FloatOfBits,
        Builtin::F32Of,
        Builtin::FloatOf32,
        Builtin::BitsOf32,
        Builtin::F32OfBits,
        Builtin::Sqrt32,
        Builtin::F32OfInt,
        Builtin::IntOfF32,
        Builtin::FsRead,
        Builtin::FsWrite,
        Builtin::OpenRead,
        Builtin::ReadFile,
        Builtin::Close,
        Builtin::OpenAppend,
        Builtin::OpenWrite,
        Builtin::OpenNew,
        Builtin::OpenRw,
        Builtin::FileWrite,
        Builtin::FilePwrite,
        Builtin::FilePread,
        Builtin::FileSync,
        Builtin::FileTruncate,
        Builtin::FileSize,
        Builtin::FsRename,
        Builtin::FsRemove,
        Builtin::FileLock,
        Builtin::Box,
        Builtin::Unbox,
        Builtin::Contents,
        Builtin::ArgCount,
        Builtin::Arg,
        Builtin::BoxSlice,
        Builtin::UnboxSlice,
        Builtin::Connect,
        Builtin::Bind,
        Builtin::Listen,
        Builtin::Accept,
        Builtin::TcpListen,
        Builtin::TcpConnect,
        Builtin::TcpConnectStart,
        Builtin::TcpAccept,
        Builtin::PollerNew,
        Builtin::PollerAddListener,
        Builtin::PollerAddConn,
        Builtin::PollerModify,
        Builtin::PollerRemove,
        Builtin::PollerWait,
        Builtin::PollerClose,
        Builtin::ClockMs,
        Builtin::ClockUnixMs,
        Builtin::ConnDetach,
        Builtin::ConnAttach,
        Builtin::ConnRead,
        Builtin::ConnWrite,
        Builtin::ConnNonblocking,
        Builtin::ConnNodelay,
        Builtin::ConnPeer,
        Builtin::ConnConnectStatus,
        Builtin::ListenerNonblocking,
        Builtin::ConnClose,
        Builtin::ListenerClose,
        Builtin::UdpConnect,
        Builtin::UdpBind,
        Builtin::UdpRecvFrom,
        Builtin::UdpSendTo,
        Builtin::UdpRecv,
        Builtin::UdpSend,
        Builtin::UdpLocalPort,
        Builtin::UdpPeer,
        Builtin::UdpNonblocking,
        Builtin::UdpClose,
        Builtin::UdpDetach,
        Builtin::UdpAttach,
        Builtin::PollerAddUdp,
        Builtin::TtyOpen,
        Builtin::TtyConfigure,
        Builtin::TtyRead,
        Builtin::TtyWrite,
        Builtin::TtyFlushInput,
        Builtin::TtyClose,
        Builtin::PollerAddTty,
        Builtin::SignalsWatch,
        Builtin::SignalsPending,
        Builtin::PollerAddSignals,
        Builtin::SignalsClose,
        Builtin::OpenDir,
        Builtin::DirEnter,
        Builtin::DirOpenRead,
        Builtin::DirClose,
        Builtin::DirOpenNew,
        Builtin::DirOpenAppend,
        Builtin::DirRename,
        Builtin::DirRenameNew,
        Builtin::DirRemove,
        Builtin::DirSync,
        Builtin::DirList,
        Builtin::DirNext,
        Builtin::DirListClose,
        Builtin::DirStat,
        Builtin::DirMode,
        Builtin::DirOwnMode,
        Builtin::PipeOpen,
        Builtin::ExecSpawn,
        Builtin::ExecSpawnIn,
        Builtin::ChildWait,
        Builtin::ChildKill,
        Builtin::PipeRead,
        Builtin::PipeWrite,
        Builtin::PipeNonblocking,
        Builtin::PipeClose,
        Builtin::ChildEndClose,
        Builtin::PollerAddPipe,
        Builtin::PollerAddChild,
        Builtin::NullPtr,
        Builtin::Spawn,
        Builtin::Join,
        Builtin::Trap,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Builtin::PutChar => "putchar",
            Builtin::Write => "write_bytes",
            Builtin::FlushOut => "flush_out",
            Builtin::WriteErr => "write_err",
            Builtin::GetChar => "getchar",
            Builtin::Split => "split",
            Builtin::Release => "release",
            Builtin::Narrow => "narrow",
            Builtin::ForkHeap => "fork_heap",
            Builtin::ForkClock => "fork_clock",
            Builtin::CopyWithin => "copy_within",
            Builtin::CopyInto => "copy_into",
            Builtin::IndexOfByte => "index_of_byte",
            Builtin::WrappingAdd => "wrapping_add",
            Builtin::WrappingSub => "wrapping_sub",
            Builtin::WrappingMul => "wrapping_mul",
            Builtin::ValueBarrier => "value_barrier",
            Builtin::HwAesGcm => "hw_aes_gcm",
            Builtin::AesEncryptBlock => "aes_encrypt_block",
            Builtin::GhashUpdate => "ghash_update",
            Builtin::Len => "len",
            Builtin::ByteOf => "byte_of",
            Builtin::IntOf => "int_of",
            Builtin::FloatOf => "float_of",
            Builtin::Truncate => "truncate",
            Builtin::IsNan => "is_nan",
            Builtin::Sqrt => "sqrt",
            Builtin::BitsOf => "bits_of",
            Builtin::FloatOfBits => "float_of_bits",
            Builtin::F32Of => "f32_of",
            Builtin::FloatOf32 => "float_of32",
            Builtin::BitsOf32 => "bits_of32",
            Builtin::F32OfBits => "f32_of_bits",
            Builtin::Sqrt32 => "sqrt32",
            Builtin::F32OfInt => "f32_of_int",
            Builtin::IntOfF32 => "int_of_f32",
            Builtin::FsRead => "fs_read",
            Builtin::OpenRead => "open_read",
            Builtin::ReadFile => "file_read",
            Builtin::Close => "file_close",
            Builtin::OpenAppend => "open_append",
            Builtin::OpenWrite => "open_write",
            Builtin::OpenNew => "open_new",
            Builtin::OpenRw => "open_rw",
            Builtin::FileWrite => "file_write",
            Builtin::FilePwrite => "file_pwrite",
            Builtin::FilePread => "file_pread",
            Builtin::FileSync => "file_sync",
            Builtin::FileTruncate => "file_truncate",
            Builtin::FileSize => "file_size",
            Builtin::FsRename => "fs_rename",
            Builtin::FsRemove => "fs_remove",
            Builtin::FileLock => "file_lock",
            Builtin::FsWrite => "fs_write",
            Builtin::Box => "box",
            Builtin::Unbox => "unbox",
            Builtin::Contents => "contents",
            Builtin::ArgCount => "arg_count",
            Builtin::Arg => "arg",
            Builtin::BoxSlice => "box_slice",
            Builtin::UnboxSlice => "unbox_slice",
            Builtin::Connect => "connect",
            Builtin::Bind => "bind",
            Builtin::Listen => "listen",
            Builtin::Accept => "accept",
            Builtin::TcpListen => "tcp_listen",
            Builtin::TcpConnect => "tcp_connect",
            Builtin::TcpConnectStart => "tcp_connect_start",
            Builtin::TcpAccept => "tcp_accept",
            Builtin::PollerNew => "poller_new",
            Builtin::PollerAddListener => "poller_add_listener",
            Builtin::PollerAddConn => "poller_add_conn",
            Builtin::PollerModify => "poller_modify",
            Builtin::PollerRemove => "poller_remove",
            Builtin::PollerWait => "poller_wait",
            Builtin::PollerClose => "poller_close",
            Builtin::ClockMs => "clock_ms",
            Builtin::ClockUnixMs => "clock_unix_ms",
            Builtin::ConnDetach => "conn_detach",
            Builtin::ConnAttach => "conn_attach",
            Builtin::ConnRead => "conn_read",
            Builtin::ConnWrite => "conn_write",
            Builtin::ConnNonblocking => "conn_nonblocking",
            Builtin::ConnNodelay => "conn_nodelay",
            Builtin::ConnPeer => "conn_peer",
            Builtin::ConnConnectStatus => "conn_connect_status",
            Builtin::ListenerNonblocking => "listener_nonblocking",
            Builtin::ConnClose => "conn_close",
            Builtin::ListenerClose => "listener_close",
            Builtin::UdpConnect => "udp_connect",
            Builtin::UdpBind => "udp_bind",
            Builtin::UdpRecvFrom => "udp_recv_from",
            Builtin::UdpSendTo => "udp_send_to",
            Builtin::UdpRecv => "udp_recv",
            Builtin::UdpSend => "udp_send",
            Builtin::UdpLocalPort => "udp_local_port",
            Builtin::UdpPeer => "udp_peer",
            Builtin::UdpNonblocking => "udp_nonblocking",
            Builtin::UdpClose => "udp_close",
            Builtin::UdpDetach => "udp_detach",
            Builtin::UdpAttach => "udp_attach",
            Builtin::PollerAddUdp => "poller_add_udp",
            Builtin::TtyOpen => "tty_open",
            Builtin::TtyConfigure => "tty_configure",
            Builtin::TtyRead => "tty_read",
            Builtin::TtyWrite => "tty_write",
            Builtin::TtyFlushInput => "tty_flush_input",
            Builtin::TtyClose => "tty_close",
            Builtin::PollerAddTty => "poller_add_tty",
            Builtin::SignalsWatch => "signals_watch",
            Builtin::SignalsPending => "signals_pending",
            Builtin::PollerAddSignals => "poller_add_signals",
            Builtin::SignalsClose => "signals_close",
            Builtin::OpenDir => "open_dir",
            Builtin::DirEnter => "dir_enter",
            Builtin::DirOpenRead => "dir_open_read",
            Builtin::DirClose => "dir_close",
            Builtin::DirOpenNew => "dir_open_new",
            Builtin::DirOpenAppend => "dir_open_append",
            Builtin::DirRename => "dir_rename",
            Builtin::DirRenameNew => "dir_rename_new",
            Builtin::DirRemove => "dir_remove",
            Builtin::DirSync => "dir_sync",
            Builtin::DirList => "dir_list",
            Builtin::DirNext => "dir_next",
            Builtin::DirListClose => "dir_list_close",
            Builtin::DirStat => "dir_stat",
            Builtin::DirMode => "dir_mode",
            Builtin::DirOwnMode => "dir_own_mode",
            Builtin::PipeOpen => "pipe_open",
            Builtin::ExecSpawn => "exec_spawn",
            Builtin::ExecSpawnIn => "exec_spawn_in",
            Builtin::ChildWait => "child_wait",
            Builtin::ChildKill => "child_kill",
            Builtin::PipeRead => "pipe_read",
            Builtin::PipeWrite => "pipe_write",
            Builtin::PipeNonblocking => "pipe_nonblocking",
            Builtin::PipeClose => "pipe_close",
            Builtin::ChildEndClose => "child_end_close",
            Builtin::PollerAddPipe => "poller_add_pipe",
            Builtin::PollerAddChild => "poller_add_child",
            Builtin::NullPtr => "null_ptr",
            Builtin::Spawn => "spawn",
            Builtin::Join => "join",
            Builtin::Trap => "trap",
        }
    }

    /// The edition a file must be at to name this builtin
    /// (`docs/editions.md` §7). `1` for every builtin that predates
    /// editions; `Net`'s are the first to answer `2`.
    ///
    /// This is what keeps `connect` from shadowing the `extern fn connect`
    /// an edition-1 file may already declare against libc, the way
    /// `examples/fetch/` does today: name resolution only answers this
    /// builtin when the calling file's edition is at least this one, so to
    /// an earlier file the name is not a builtin at all.
    pub fn since(self) -> u32 {
        match self {
            Builtin::Connect | Builtin::Bind | Builtin::Listen | Builtin::Accept => 2,
            // `docs/opaque-pointers.md` §4: purely additive, the same
            // reason `Net`'s builtins needed edition 2 rather than
            // silently widening edition 1 -- an edition-1 file may
            // already declare its own `extern fn null_ptr`.
            Builtin::NullPtr => 3,
            // `docs/threads.md` §4: purely additive, same reasoning.
            Builtin::Spawn | Builtin::Join | Builtin::ForkHeap => 4,
            // `docs/native-sockets.md` §3: edition 5, and for the same
            // reason -- `conn_read` is a name an edition-1 file may
            // already declare against libc.
            Builtin::TcpListen
            | Builtin::TcpConnect
            | Builtin::TcpConnectStart
            | Builtin::TcpAccept
            | Builtin::PollerNew
            | Builtin::PollerAddListener
            | Builtin::PollerAddConn
            | Builtin::PollerModify
            | Builtin::PollerRemove
            | Builtin::PollerWait
            | Builtin::PollerClose
            | Builtin::ClockMs
            | Builtin::ClockUnixMs
            | Builtin::ConnDetach
            | Builtin::ConnAttach
            | Builtin::ConnRead
            | Builtin::ConnWrite
            | Builtin::ConnNonblocking
            | Builtin::ConnNodelay
            | Builtin::ConnPeer
            | Builtin::ConnConnectStatus
            | Builtin::ListenerNonblocking
            | Builtin::ConnClose
            | Builtin::ForkClock
            | Builtin::CopyWithin
            | Builtin::CopyInto
            | Builtin::IndexOfByte
            | Builtin::ListenerClose
            // `docs/udp.md` §3: edition 5, for the reason the TCP verbs are.
            | Builtin::UdpConnect
            | Builtin::UdpBind
            | Builtin::UdpRecvFrom
            | Builtin::UdpSendTo
            | Builtin::UdpRecv
            | Builtin::UdpSend
            | Builtin::UdpLocalPort
            | Builtin::UdpPeer
            | Builtin::UdpNonblocking
            | Builtin::UdpClose
            | Builtin::UdpDetach
            | Builtin::UdpAttach
            | Builtin::PollerAddUdp => 5,
            // `docs/tty.md` §6: edition 8, the same reasoning as every
            // capability's verbs -- the names are ones a program may
            // already have declared for itself through `extern fn`.
            Builtin::TtyOpen
            | Builtin::TtyConfigure
            | Builtin::TtyRead
            | Builtin::TtyWrite
            | Builtin::TtyFlushInput
            | Builtin::TtyClose
            | Builtin::PollerAddTty => 8,
            // `docs/checked-output.md`: a name a program may already have
            // declared for itself, so it is visible from edition 5 only.
            Builtin::FlushOut => 5,
            // `docs/value-barrier.md` §3: edition 6, the latest, for the
            // same reason -- a program may already declare the name.
            Builtin::ValueBarrier => 6,
            // `docs/f32.md` §6: edition 6, the latest, like `value_barrier`
            // -- a program may already declare `f32_of`. None in this
            // repository does (counted there), so no edition 7 is made.
            Builtin::F32Of
            | Builtin::FloatOf32
            | Builtin::BitsOf32
            | Builtin::F32OfBits
            | Builtin::Sqrt32
            | Builtin::F32OfInt
            | Builtin::IntOfF32 => 6,
            // `docs/signals.md`: edition 6, for the same reason --
            // `signals_watch` is a name a program may already declare.
            Builtin::SignalsWatch
            | Builtin::SignalsPending
            | Builtin::PollerAddSignals
            | Builtin::SignalsClose => 6,
            // `docs/directory-handles.md`: edition 6 -- `open_dir` and the
            // `dir_*` names are ones a program may already declare.
            Builtin::OpenDir
            | Builtin::DirEnter
            | Builtin::DirOpenRead
            | Builtin::DirClose
            | Builtin::DirOpenNew
            | Builtin::DirOpenAppend
            | Builtin::DirRename
            | Builtin::DirRemove
            | Builtin::DirSync
            | Builtin::DirList
            | Builtin::DirNext
            | Builtin::DirListClose
            | Builtin::DirStat => 6,
            // `docs/processes.md`: edition 7 -- `pipe_open` and `child_wait`
            // are names a program may already declare.
            Builtin::PipeOpen
            | Builtin::ExecSpawn
            | Builtin::ExecSpawnIn
            | Builtin::ChildWait
            | Builtin::ChildKill
            | Builtin::PipeRead
            | Builtin::PipeWrite
            | Builtin::PipeNonblocking
            | Builtin::PipeClose
            | Builtin::ChildEndClose
            | Builtin::PollerAddPipe
            | Builtin::PollerAddChild => 7,
            // `docs/directory-listing.md` §3.5: edition 7, which is still
            // being built -- `dir_mode` is a name a program may already
            // declare.
            Builtin::DirMode | Builtin::DirOwnMode => 7,
            // `docs/directory-handles.md` §3, slice 4: edition 7 as well --
            // `dir_rename_new` is a name a program may already declare.
            Builtin::DirRenameNew => 7,
            // `docs/crypto-builtins.md` §3: the latest edition, as
            // `value_barrier` was, since a program may already declare
            // these names.
            Builtin::HwAesGcm | Builtin::AesEncryptBlock | Builtin::GhashUpdate => 7,
            // `docs/floating-point.md` §4.1: edition 7, the latest -- the
            // first caller was a table reader that decoded doubles through
            // `ldexp`; `float_of_bits` is a name a program may declare.
            Builtin::FloatOfBits => 7,
            // `docs/file-writes.md`: edition 5, for the same reason --
            // `file_write` and `open_new` are names a program may already
            // declare against libc.
            Builtin::OpenAppend
            | Builtin::OpenWrite
            | Builtin::OpenNew
            | Builtin::OpenRw
            | Builtin::FileWrite
            | Builtin::FilePwrite
            | Builtin::FilePread
            | Builtin::FileSync
            | Builtin::FileTruncate
            | Builtin::FileSize
            | Builtin::FsRename
            | Builtin::FsRemove
            | Builtin::FileLock => 5,
            _ => 1,
        }
    }

    /// The libc symbol the backend calls, for the ones that reach libc.
    ///
    /// `split` and `release` reach nothing: they are the ceremony that moves
    /// authority around, and authority erases (§8.1). The backend emits no
    /// call for them at all.
    pub fn symbol(self) -> Option<&'static str> {
        match self {
            Builtin::PutChar => Some("putchar"),
            // `fwrite`, not POSIX `write`: `putchar` goes through stdio,
            // and a bulk write on a raw descriptor would interleave
            // wrongly with it. The stream has to be the same one.
            Builtin::Write => Some("fwrite"),
            // The same call on the other stream. C guarantees `stderr`
            // is not fully buffered, which is what makes a diagnostic
            // written just before a trap arrive at all
            // (`docs/standard-error.md` §1.2).
            Builtin::WriteErr => Some("fwrite"),
            Builtin::GetChar => Some("getchar"),
            _ => None,
        }
    }

    /// How many leading arguments carry authority rather than data, and so
    /// do not reach the foreign function underneath.
    ///
    /// `putchar`'s `Io` is a *borrowed* capability, which is one pointer at
    /// a zero-sized value — real enough for the checker to track and
    /// meaningless to libc, which wants the character and nothing else.
    /// Passing it along made libc print the pointer.
    pub fn erased_args(self) -> usize {
        match self {
            // Each of these takes a borrowed capability first. It is
            // leaf-free, so it contributes no values either way, and
            // skipping it keeps the argument positions honest.
            Builtin::PutChar | Builtin::GetChar | Builtin::ArgCount | Builtin::Arg => 1,
            Builtin::Write | Builtin::WriteErr | Builtin::FlushOut => 1,
            _ => 0,
        }
    }

    /// How many region parameters the builtin takes, so a call site can
    /// instantiate them the same way it does for a written function (§5.1).
    ///
    /// A builtin that forgets to count one here keeps `Region::Param(0)`
    /// *rigid*, and then no caller's block region can ever unify with it —
    /// the call works from inside a region-polymorphic function and fails
    /// from inside a `borrow` block, which is a confusing way to find out.
    pub fn regions(self) -> usize {
        match self {
            Builtin::PutChar
            | Builtin::GetChar
            | Builtin::FlushOut
            | Builtin::ArgCount
            | Builtin::Arg => 1,
            // Two: the borrowed `Io` and the slice's own region.
            Builtin::Write | Builtin::WriteErr => 2,
            // Two: the borrowed handle and the buffer's own region.
            Builtin::ReadFile => 2,
            // The handle's region and the buffer's.
            Builtin::FileWrite | Builtin::FilePwrite | Builtin::FilePread => 2,
            // The handle's region alone.
            Builtin::FileSync | Builtin::FileTruncate | Builtin::FileSize | Builtin::FileLock => 1,
            // The handle's region, and for `conn_read`/`conn_write` the
            // buffer's own.
            Builtin::ConnRead
            | Builtin::ConnWrite
            | Builtin::ConnPeer
            | Builtin::UdpRecv
            | Builtin::UdpSend
            | Builtin::UdpPeer
            | Builtin::UdpSendTo => 2,
            // The handle's region, the buffer's, and the ticket cell's.
            Builtin::UdpRecvFrom => 3,
            // The poller's region and the handle's (or the buffer's).
            Builtin::PollerAddListener
            | Builtin::PollerAddConn
            | Builtin::PollerAddUdp
            | Builtin::PollerModify
            | Builtin::PollerRemove
            | Builtin::PollerWait
            | Builtin::PollerAddSignals
            | Builtin::PollerAddTty => 2,
            // The capability's region and the path's.
            Builtin::TtyOpen => 2,
            // The handle's region and the buffer's (or the capability's
            // and the path's, for `tty_open`).
            Builtin::TtyRead | Builtin::TtyWrite => 2,
            // Only the handle's.
            Builtin::TtyConfigure | Builtin::TtyFlushInput => 1,
            Builtin::SignalsPending => 1,
            // The handle's region and the name's.
            Builtin::DirEnter
            | Builtin::DirOpenRead
            | Builtin::DirOpenNew
            | Builtin::DirOpenAppend => 2,
            Builtin::DirRemove => 2,
            // The handle's region and each name's.
            Builtin::DirRename | Builtin::DirRenameNew => 3,
            Builtin::DirSync => 1,
            // The handle's region; for `dir_next`, the listing's and the
            // buffer's.
            Builtin::DirList => 1,
            Builtin::DirNext => 2,
            // The handle's region and the name's.
            Builtin::DirStat => 2,
            Builtin::DirMode => 2,
            // The handle's region.
            Builtin::DirOwnMode => 1,
            // The handle's region, and for a read or a write the buffer's.
            Builtin::PipeRead | Builtin::PipeWrite => 2,
            Builtin::ChildKill | Builtin::PipeNonblocking => 1,
            // The poller's region and the handle's.
            Builtin::PollerAddPipe | Builtin::PollerAddChild => 2,
            Builtin::TcpAccept
            | Builtin::ConnNonblocking
            | Builtin::ConnNodelay
            | Builtin::ConnConnectStatus
            | Builtin::ListenerNonblocking
            | Builtin::UdpLocalPort
            | Builtin::UdpNonblocking
            | Builtin::ForkClock
            | Builtin::CopyWithin
            | Builtin::IndexOfByte
            | Builtin::ClockMs
            | Builtin::ClockUnixMs => 1,
            // The destination's region and the source's.
            Builtin::CopyInto => 2,
            // The key's, the block's and the output's; `h`'s, `y`'s and
            // the data's.
            Builtin::AesEncryptBlock | Builtin::GhashUpdate => 3,
            _ => 0,
        }
    }

    /// What performing this builtin costs a caller's row.
    ///
    /// `putchar` writes to the console, so it performs `io_write`. This is the
    /// *grounding* of the whole system: every `io_write` in every row above it
    /// traces back here, because a label nothing performs can never appear
    /// in an exact row (§7.3).
    pub fn effects(self) -> Effects {
        match self {
            Builtin::PutChar | Builtin::Write | Builtin::FlushOut => Effects::plain(["io_write"]),
            // Its own label rather than `io_write`, because the stream is
            // the unit a reader can act on: `1>` and `2>` are two
            // redirections (`docs/standard-error.md` §3.1).
            Builtin::WriteErr => Effects::plain(["err_write"]),
            // `docs/standard-input.md` §2: the same capability, the other
            // direction, its own label. A row saying `[io_write]` does not
            // permit a read, which is what makes the two labels a
            // distinction rather than a spelling.
            Builtin::GetChar => Effects::plain(["io_read"]),
            // `docs/heap.md` §2. Both reach the allocator, so both perform
            // `heap`; `contents` is a load and performs nothing.
            Builtin::Box
            | Builtin::Unbox
            | Builtin::BoxSlice
            | Builtin::UnboxSlice
            | Builtin::ForkHeap => Effects::plain(["heap"]),
            // §2: reading the command line is an effect, because a
            // function whose behaviour depends on it should say so.
            Builtin::ArgCount | Builtin::Arg => Effects::plain(["args"]),
            // `docs/file-handles.md` §4.1: a path-free label, because the
            // path was spent at `open_read` and the row there still names
            // the directory. `close` performs nothing for the same reason
            // `release` does not -- ending a capability is not using one --
            // even though this one ends with a syscall.
            Builtin::ReadFile | Builtin::FilePread | Builtin::FileSize => {
                Effects::plain(["file_read"])
            }
            // `docs/file-writes.md` section 4: the same path-free rule on
            // the write side. A sync is `file_write`, the conservative
            // label (section 5.2).
            Builtin::FileWrite
            | Builtin::FilePwrite
            | Builtin::FileSync
            | Builtin::FileTruncate
            | Builtin::FileLock => Effects::plain(["file_write"]),
            // `docs/native-sockets.md` §3: path-free labels named after the
            // handle and the direction; `tcp_listen`'s row comes from the
            // bound at the call site.
            Builtin::TcpAccept => Effects::plain(["conn_accept"]),
            Builtin::ConnRead => Effects::plain(["conn_read"]),
            // `docs/udp.md` §3: path-free, the peer was spent at `udp_connect`.
            Builtin::UdpRecv | Builtin::UdpRecvFrom => Effects::plain(["udp_recv"]),
            Builtin::UdpSend | Builtin::UdpSendTo => Effects::plain(["udp_send"]),
            Builtin::ClockMs | Builtin::ClockUnixMs => Effects::plain(["clock"]),
            Builtin::PollerAddListener
            | Builtin::PollerAddConn
            | Builtin::PollerAddUdp
            | Builtin::PollerModify
            | Builtin::PollerRemove
            | Builtin::PollerWait
            | Builtin::PollerAddPipe
            | Builtin::PollerAddChild
            | Builtin::PollerAddSignals
            | Builtin::PollerAddTty => Effects::plain(["poll"]),
            // `docs/tty.md` §4: argument-carrying like `fs_read`, the
            // path under the capability's bound. The bound is the
            // capability's own, spent at `tty_open` like `udp_connect`
            // spends the `Net`'s, so the port's own verbs carry no
            // argument -- a `Conn`'s shape.
            // `docs/tty.md` §4: `tty_open` is lowered as `Expr::TtyOpen`,
            // whose own walk records the label with the capability's
            // prefix (`tcp_listen`'s shape) — the path is a run-time
            // value, the bound is in the capability's type.
            Builtin::TtyOpen => Effects::pure(),
            // The port's own verbs are path-free, the prefix spent at
            // `tty_open` (`udp_recv`'s shape): the *handle* carries no
            // argument, and it is reached only through the capability.
            Builtin::TtyRead => Effects::plain(["tty_read"]),
            Builtin::TtyWrite => Effects::plain(["tty_write"]),
            // Configuring, flushing and closing touch no new domain:
            // the port is already open under the bound, the same reason
            // `udp_close` performs nothing.
            Builtin::TtyConfigure | Builtin::TtyFlushInput | Builtin::TtyClose => Effects::pure(),
            // `docs/signals.md` section 2.1: path-free, the set was spent at
            // `signals_watch`. Closing performs nothing, as `conn_close` does not.
            Builtin::SignalsPending => Effects::plain(["signals_read"]),
            // `docs/directory-handles.md` §2: the handle is the authority.
            Builtin::DirEnter | Builtin::DirOpenRead => Effects::plain(["dir_read"]),
            // `docs/directory-listing.md` §3.3: listing is reading beneath the
            // directory, and closing a listing performs nothing.
            Builtin::DirList | Builtin::DirNext | Builtin::DirStat => Effects::plain(["dir_read"]),
            // §3.5: a status, so a read beneath the directory, as `dir_stat`.
            Builtin::DirMode | Builtin::DirOwnMode => Effects::plain(["dir_read"]),
            // §3: everything that changes what is beneath a directory.
            Builtin::DirOpenNew
            | Builtin::DirOpenAppend
            | Builtin::DirRename
            | Builtin::DirRenameNew
            | Builtin::DirRemove
            | Builtin::DirSync => Effects::plain(["dir_write"]),
            Builtin::ConnWrite => Effects::plain(["conn_write"]),
            // `docs/processes.md` §3.2: path-free, the program was named at
            // `exec_spawn`. Waiting and closing perform nothing, as
            // `conn_close` does not; `exec_spawn`'s row comes from the prefix
            // at the call site.
            Builtin::ChildKill => Effects::plain(["child_signal"]),
            Builtin::PipeRead => Effects::plain(["pipe_read"]),
            Builtin::PipeWrite => Effects::plain(["pipe_write"]),
            // Moving authority around is not an effect. Splitting a `World`
            // observes nothing outside the program and releasing a
            // capability only ends one; what a capability *authorises* is
            // where the effect is.
            // Arithmetic is not an effect, wrapping or not: it observes
            // nothing outside the program and needs no authority.
            _ => Effects::pure(),
        }
    }

    pub fn from_name(name: &str) -> Option<Builtin> {
        Builtin::ALL.iter().copied().find(|b| b.name() == name)
    }
}
