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
    /// lex-sys, rather than a hole in the standard library.
    BitsOf,
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
    /// correctly-rounded square root cannot be written in lex-sys.** The
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
}

impl Builtin {
    pub const ALL: &'static [Builtin] = &[
        Builtin::PutChar,
        Builtin::Write,
        Builtin::WriteErr,
        Builtin::GetChar,
        Builtin::Split,
        Builtin::Release,
        Builtin::Narrow,
        Builtin::WrappingAdd,
        Builtin::WrappingSub,
        Builtin::WrappingMul,
        Builtin::Len,
        Builtin::ByteOf,
        Builtin::IntOf,
        Builtin::FloatOf,
        Builtin::Truncate,
        Builtin::IsNan,
        Builtin::Sqrt,
        Builtin::BitsOf,
        Builtin::FsRead,
        Builtin::FsWrite,
        Builtin::OpenRead,
        Builtin::ReadFile,
        Builtin::Close,
        Builtin::Box,
        Builtin::Unbox,
        Builtin::Contents,
        Builtin::ArgCount,
        Builtin::Arg,
        Builtin::BoxSlice,
        Builtin::UnboxSlice,
        Builtin::Connect,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Builtin::PutChar => "putchar",
            Builtin::Write => "write_bytes",
            Builtin::WriteErr => "write_err",
            Builtin::GetChar => "getchar",
            Builtin::Split => "split",
            Builtin::Release => "release",
            Builtin::Narrow => "narrow",
            Builtin::WrappingAdd => "wrapping_add",
            Builtin::WrappingSub => "wrapping_sub",
            Builtin::WrappingMul => "wrapping_mul",
            Builtin::Len => "len",
            Builtin::ByteOf => "byte_of",
            Builtin::IntOf => "int_of",
            Builtin::FloatOf => "float_of",
            Builtin::Truncate => "truncate",
            Builtin::IsNan => "is_nan",
            Builtin::Sqrt => "sqrt",
            Builtin::BitsOf => "bits_of",
            Builtin::FsRead => "fs_read",
            Builtin::OpenRead => "open_read",
            Builtin::ReadFile => "file_read",
            Builtin::Close => "file_close",
            Builtin::FsWrite => "fs_write",
            Builtin::Box => "box",
            Builtin::Unbox => "unbox",
            Builtin::Contents => "contents",
            Builtin::ArgCount => "arg_count",
            Builtin::Arg => "arg",
            Builtin::BoxSlice => "box_slice",
            Builtin::UnboxSlice => "unbox_slice",
            Builtin::Connect => "connect",
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
            Builtin::Connect => 2,
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
            Builtin::Write | Builtin::WriteErr => 1,
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
            Builtin::PutChar | Builtin::GetChar | Builtin::ArgCount | Builtin::Arg => 1,
            // Two: the borrowed `Io` and the slice's own region.
            Builtin::Write | Builtin::WriteErr => 2,
            // Two: the borrowed handle and the buffer's own region.
            Builtin::ReadFile => 2,
            _ => 0,
        }
    }

    /// Parameter types and return type, in terms of the prelude's ids.
    ///
    /// `prelude` is `[World, Io, Split]` — the ids `collect_types` handed
    /// out, which are fixed because the prelude is collected first.
    pub fn signature(self, prelude: &[DefId]) -> (Vec<Type>, Type) {
        let named = |i: usize| Type::Named(prelude[i], Vec::new());
        match self {
            Builtin::PutChar => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_IO)),
                    },
                    Type::Int,
                ],
                Type::Int,
            ),
            // The same borrowed `Io`, and a shared slice of the bytes.
            // Shared rather than unique because writing reads them, and
            // `strings.md` §4's coercion lets a caller hand over a
            // unique one anyway.
            //
            // `write_err` is the same signature on the other stream, so
            // it shares this arm rather than repeating it: a difference
            // between them here would be a difference nothing asked for
            // (`docs/standard-error.md` §3).
            Builtin::Write | Builtin::WriteErr => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_IO)),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                Type::Int,
            ),
            // The mirror: the same borrowed `Io`, no character to take.
            Builtin::GetChar => (
                vec![Type::Ref {
                    unique: true,
                    region: Region::Param(0),
                    inner: Box::new(named(PRELUDE_IO)),
                }],
                Type::Int,
            ),
            // Checked at the call site (`docs/editions.md` §7): the return
            // type depends on the caller's edition, and a fixed signature
            // cannot say that.
            Builtin::Split => (Vec::new(), Type::Unit),
            Builtin::WrappingAdd | Builtin::WrappingSub | Builtin::WrappingMul => {
                (vec![Type::Int, Type::Int], Type::Int)
            }
            Builtin::Len => (Vec::new(), Type::Int),
            // Both are checked at the call site: the prefix in the
            // capability's type is what decides the row, and a fixed
            // signature cannot say that.
            Builtin::FsRead | Builtin::FsWrite => (Vec::new(), Type::Unit),
            // Checked at the call site, exactly as `fs_read` is: the prefix
            // is in the capability's type (`docs/file-handles.md` §2.1).
            Builtin::OpenRead => (Vec::new(), Type::Unit),
            // The handle is borrowed uniquely because the read moves the
            // descriptor's offset, and the buffer uniquely because the read
            // writes into it -- the same pair `fs_read` takes, with the
            // capability replaced by the handle it was spent on.
            Builtin::ReadFile => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_FILE)),
                    },
                    Type::Ref {
                        unique: true,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                named(PRELUDE_READ),
            ),
            // By value: `close` ends the handle, which is what `res` means.
            Builtin::Close => (vec![named(PRELUDE_FILE)], Type::Int),
            // All three depend on the type being boxed, which a fixed
            // signature has no parameter to name (`docs/heap.md` §3).
            Builtin::Box
            | Builtin::Unbox
            | Builtin::Contents
            | Builtin::BoxSlice
            | Builtin::UnboxSlice => (Vec::new(), Type::Unit),
            // `docs/arguments.md` §3. Written out rather than checked at
            // the call site, because neither depends on a type the caller
            // chose: an argument is always `&static [byte]`.
            Builtin::ArgCount => (
                vec![Type::Ref {
                    unique: false,
                    region: Region::Param(0),
                    inner: Box::new(named(PRELUDE_ARGS)),
                }],
                Type::Int,
            ),
            Builtin::Arg => (
                vec![
                    Type::Ref {
                        unique: false,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_ARGS)),
                    },
                    Type::Int,
                ],
                Type::Ref {
                    unique: false,
                    region: Region::Static,
                    inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                },
            ),
            Builtin::ByteOf => (vec![Type::Int], Type::Byte),
            Builtin::IntOf => (vec![Type::Byte], Type::Int),
            Builtin::FloatOf => (vec![Type::Int], Type::Float),
            Builtin::Truncate => (vec![Type::Float], Type::Int),
            Builtin::IsNan => (vec![Type::Float], Type::Bool),
            // Float in, float out, and nothing else: no capability, because
            // it reaches no library (`docs/float-math.md` §3).
            Builtin::Sqrt => (vec![Type::Float], Type::Float),
            Builtin::BitsOf => (vec![Type::Float], Type::Int),
            // Both are checked at the call site rather than here, because a
            // fixed signature cannot say what they need. `release` ends any
            // capability, and there is more than one kind; `narrow` has an
            // argument *and* a result that depend on the literal written at
            // the call.
            Builtin::Release | Builtin::Narrow => (Vec::new(), Type::Unit),
            // Checked at the call site, exactly as `fs_read` is: the bound
            // is in the capability's type, and a fixed signature cannot
            // say that (`docs/net.md` §4.1).
            Builtin::Connect => (Vec::new(), Type::Unit),
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
            Builtin::PutChar | Builtin::Write => Effects::plain(["io_write"]),
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
            Builtin::Box | Builtin::Unbox | Builtin::BoxSlice | Builtin::UnboxSlice => {
                Effects::plain(["heap"])
            }
            // §2: reading the command line is an effect, because a
            // function whose behaviour depends on it should say so.
            Builtin::ArgCount | Builtin::Arg => Effects::plain(["args"]),
            // `docs/file-handles.md` §4.1: a path-free label, because the
            // path was spent at `open_read` and the row there still names
            // the directory. `close` performs nothing for the same reason
            // `release` does not -- ending a capability is not using one --
            // even though this one ends with a syscall.
            Builtin::ReadFile => Effects::plain(["file_read"]),
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
