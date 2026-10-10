//! The IR itself: slots, effects, expressions, statements, functions
//! and the program -- what lowering produces and the backend reads.

use crate::*;

/// A local variable. Parameters occupy slots `0..n_params`; each `let`/`var`
/// takes the next slot and never reuses one, so a shadowing binding is simply
/// a different slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot(pub u32);

/// Index into [`Program::funcs`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuncId(pub u32);

/// Positions in the prelude's type table, which `collect_types` builds
/// first so these are the same in every program.
pub const PRELUDE_WORLD: usize = 0;
pub const PRELUDE_IO: usize = 1;
pub const PRELUDE_FFI: usize = 2;
pub const PRELUDE_FS: usize = 3;
pub const PRELUDE_HEAP: usize = 4;
pub const PRELUDE_BOX: usize = 5;
pub const PRELUDE_ARGS: usize = 6;
pub const PRELUDE_SPLIT: usize = 7;
/// `docs/file-handles.md`: an open descriptor, and the two enums its verbs
/// answer. A `File` is a capability the program *made* rather than one
/// `split` handed it — `open_read` manufactures one out of an `Fs(p)` and
/// a path, which is why that row carries the prefix and `read`'s does not
/// (§4.1).
pub const PRELUDE_FILE: usize = 8;
pub const PRELUDE_OPENED: usize = 9;
pub const PRELUDE_READ: usize = 10;
/// `docs/net.md`: the outbound capability, and `docs/editions.md` §7's
/// edition-2 `Split` that carries it. Two distinct declarations rather
/// than a sixth field added to [`PRELUDE_SPLIT`], because `Split` is
/// `res` by inference and a field nothing edition-gated could unname
/// would have to be consumed by every caller -- exactly the break
/// editions.md §7 says a program written before edition 2 must not
/// take. An edition-1 file's `Split` is still the five-field one; an
/// edition-2 file's `split()` answers this one instead.
pub const PRELUDE_NET: usize = 11;
pub const PRELUDE_SPLIT_NET: usize = 12;
/// `docs/threads.md` §2: `spawn`'s own handle, `res`, two generic
/// parameters (`T` the payload type, `R` `body`'s return type) and no
/// fields — the same "nothing to name" shape [`PRELUDE_BOX`] already
/// has, for the same reason: what it owns is an opaque thread id, and a
/// pattern that could name it would be a way to end the obligation
/// without joining.
pub const PRELUDE_THREAD: usize = 13;
/// `docs/native-sockets.md` §3, edition 5: a listening socket and an
/// established connection -- `res`, one descriptor leaf each, no literal
/// form -- and the enums their verbs answer. Appended after `Thread` so no
/// earlier index moves.
pub const PRELUDE_LISTENER: usize = 14;
pub const PRELUDE_CONN: usize = 15;
pub const PRELUDE_LISTENING: usize = 16;
pub const PRELUDE_ACCEPTED: usize = 17;
pub const PRELUDE_RECEIVED: usize = 18;
pub const PRELUDE_SENT: usize = 19;
pub const PRELUDE_DIALED: usize = 20;
/// `docs/native-sockets.md` §4, edition 5: a set of handles the kernel is
/// watching (`epoll`/`kqueue`), and what creating one answers.
pub const PRELUDE_POLLER: usize = 21;
pub const PRELUDE_POLLING: usize = 22;
/// `docs/native-sockets.md` §5, edition 5: the clock capability (leaf-free,
/// like `Io`) and the `Split` that carries it as its seventh field.
pub const PRELUDE_CLOCK: usize = 23;
pub const PRELUDE_SPLIT_CLOCK: usize = 24;
/// What `conn_attach` answers: a `Conn` redeemed from a ticket, or why the
/// ticket was refused (`docs/native-sockets.md` §10.3).
pub const PRELUDE_ATTACHED: usize = 25;
/// What a write-side file verb answers: a count (or `0`), or the `errno`
/// (`docs/file-writes.md` section 4). Edition 5.
pub const PRELUDE_DONE: usize = 26;

/// `docs/signals.md` section 2, edition 6: the signal capability (leaf-free,
/// like `Io`, indexed by the set it was narrowed to as `Net` is by a bound),
/// the `Split` that carries it as its eighth field, the claim (`res`, one
/// leaf) and what claiming answers.
pub const PRELUDE_SIGNALS: usize = 27;
pub const PRELUDE_SPLIT_SIGNALS: usize = 28;
pub const PRELUDE_SIGNAL_WATCH: usize = 29;
pub const PRELUDE_WATCHING: usize = 30;

/// `docs/directory-handles.md`, edition 6: a directory handle (`res`, one
/// leaf, like `File`) and what opening one answers.
pub const PRELUDE_DIR: usize = 31;
pub const PRELUDE_DIR_OPENED: usize = 32;

/// `docs/directory-listing.md`, edition 6: a listing in progress (`res`, one
/// leaf, the stream), what starting one answers, and what each step answers.
pub const PRELUDE_DIR_LIST: usize = 33;
pub const PRELUDE_LISTING: usize = 34;
pub const PRELUDE_LISTED: usize = 35;
/// What `dir_stat` answers (`docs/directory-listing.md` §3.2).
pub const PRELUDE_DIR_STAT: usize = 36;

/// `docs/processes.md` §3.1, edition 7: the capability to start a program
/// (leaf-free, indexed by a path prefix as `Fs` is), the `Split` that carries
/// it as its ninth field, a started child (`res`, one leaf: the pid), the
/// parent's and the child's ends of a channel (`res`, one descriptor each),
/// what one of the child's streams is, and what the verbs answer.
pub const PRELUDE_EXEC: usize = 37;
pub const PRELUDE_SPLIT_EXEC: usize = 38;
pub const PRELUDE_CHILD: usize = 39;
pub const PRELUDE_PIPE: usize = 40;
pub const PRELUDE_CHILD_END: usize = 41;
pub const PRELUDE_STDIO: usize = 42;
pub const PRELUDE_PIPED: usize = 43;
pub const PRELUDE_SPAWNED: usize = 44;
pub const PRELUDE_EXITED: usize = 45;

/// `docs/udp.md` §3, edition 5: a datagram socket (`res`, one descriptor
/// leaf, like `Conn`), what opening one answers, and what a receive answers.
/// Appended last so no earlier index moves.
pub const PRELUDE_UDP: usize = 46;
pub const PRELUDE_UDP_OPENED: usize = 47;
pub const PRELUDE_DATAGRAM: usize = 48;

/// `docs/tty.md` §3, edition 8: a serial port (`res`, one descriptor leaf,
/// like `Udp`), what opening one answers, and the ninth `Split` field that
/// carries the capability. Appended last so no earlier index moves.
pub const PRELUDE_TTY: usize = 49;
pub const PRELUDE_SPLIT_TTY: usize = 50;
pub const PRELUDE_PORT: usize = 51;
pub const PRELUDE_TTY_OPENED: usize = 52;

/// How many types the prelude declares. Written once, because a builtin's
/// signature indexes this table and a stale slice is a panic rather than a
/// diagnostic.
pub const PRELUDE_COUNT: usize = 53;

/// Which path operation an [`Expr::PathOp`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathOp {
    /// `fs_remove`: `unlink(2)`. It does not remove directories.
    Remove,
    /// `fs_rename`: `rename(2)`, both paths checked against the prefix.
    Rename,
}

/// How `open_*` opens its file (`docs/file-handles.md` section 2.1,
/// `docs/file-writes.md` section 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    /// `open_read`: `open(path, O_RDONLY)`.
    Read,
    /// `open_append`: create if missing, every write at the end (`"ab"`).
    Append,
    /// `open_write`: create or truncate (`"wb"`).
    Write,
    /// `open_new`: create, and refuse with `EEXIST` if it is there (`"wbx"`).
    New,
    /// `open_rw`: an existing file, read and write, no truncation (`"r+b"`).
    ReadWrite,
    /// `open_dir` (`docs/directory-handles.md`): `open(path, O_RDONLY |
    /// O_DIRECTORY)`, answering a `DirOpened` rather than an `Opened`.
    Directory,
}

impl OpenMode {
    /// The `fopen` mode string for a mode that is not `Read`.
    pub fn fopen_mode(self) -> &'static str {
        match self {
            OpenMode::Read => "rb",
            OpenMode::Append => "ab",
            OpenMode::Write => "wb",
            OpenMode::New => "wbx",
            OpenMode::ReadWrite => "r+b",
            // Never reaches `fopen`: both backends open a directory with
            // `open` and its own flags. Read-only is the honest mode.
            OpenMode::Directory => "rb",
        }
    }

    /// The `openat` flags that mean what [`fopen_mode`](Self::fopen_mode)'s string
    /// means, for a target whose flags are `f`: `wb` is write-only, create,
    /// truncate; `ab` is write-only, create, append; `wbx` is write-only, create,
    /// exclusive; `r+b` is read-write and nothing else. (`cloexec` and the creation
    /// mode are the caller's.)
    ///
    /// The WASI path opens a file this way rather than through `fopen` and a `dup`
    /// of its descriptor: WASI has no `dup`, `fcntl(F_DUPFD_CLOEXEC)` answers
    /// `EINVAL`, and `fopen` brings stdio's imports with it (`docs/wasm.md`).
    pub fn open_flags(self, f: &OpenFlags) -> i64 {
        match self {
            OpenMode::Read => f.read_only,
            OpenMode::Write => f.write_only | f.create | f.truncate,
            OpenMode::Append => f.write_only | f.create | f.append,
            OpenMode::New => f.write_only | f.create | f.exclusive,
            OpenMode::ReadWrite => f.read_write,
            OpenMode::Directory => f.read_only | f.directory,
        }
    }
}

/// The library an unnarrowed `Ffi` names: none of them yet.
///
/// §7.4's narrowing is prefix extension — `Fs("/var")` becomes
/// `Fs("/var/log/app")` — and the empty string is the prefix of everything,
/// so the capability `split` hands out can still become any library and no
/// narrowed one can become another.
pub const FFI_ROOT: &str = "";

/// The one region with a name rather than a binder (`docs/strings.md` §4):
/// where a string literal's bytes live, which is the object file rather
/// than any frame.
pub const STATIC_REGION: &str = "static";

/// The arena number a `static` item's own allocations carry.
///
/// Not a block, because a `static` has no `region` statement and no
/// backend ever sees one: the body is evaluated during compilation and
/// its answer becomes data (`docs/compile-time-data.md` §3). The sentinel
/// exists so `alloc_slice`'s one code path serves both.
pub(crate) const STATIC_ARENA: u32 = u32::MAX;

/// An effect row: a canonically ordered set of labels
/// (`docs/linearity-and-effects.md` §7.1).
///
/// A *set*, not a row: no duplicates and no row variables. Duplicates buy
/// handlers and masking, M2 has none, so they would be a cost with no
/// purchase — and a canonical order is what makes a signature hashable,
/// which per-unit identity is made of.
///
/// Ordered by the label's text rather than by interner index, because an
/// index is a fact about which names a *file* happened to mention first and
/// a hash must not depend on that.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Effects(Vec<Label>);

/// One label, and the value it was narrowed to (§7.4).
///
/// `ffi` and `ffi("libc")` are different labels, and the second is narrower.
/// The argument is a compile-time literal, never a runtime value, which is
/// what lets the refinement be checked structurally.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Label {
    pub name: String,
    pub argument: Option<String>,
}

impl Label {
    /// Does holding the authority for `self` also authorise `other`?
    ///
    /// The same prefix-extension rule narrowing itself uses (§7.4), and for
    /// the same reason: what a capability covers is exactly what it can be
    /// narrowed to. `ffi("")` covers every library, `ffi("libc")` covers
    /// `ffi("libc")` and nothing else, and a label with no argument covers
    /// only itself.
    ///
    /// `fs_read`, `fs_write` and `exec` name a *path*, and a path prefix is
    /// not a byte prefix (`docs/filesystem.md` §1.1): `fs_read("/tmp")` covers
    /// `fs_read("/tmp/x")` and itself, and does not cover `fs_read("/tmpevil")`.
    /// That is [`extends_path`], the rule `narrow` already checks, so what a
    /// capability covers is still exactly what it can be narrowed to.
    pub fn covers(&self, other: &Label) -> bool {
        self.name == other.name
            && match (&self.argument, &other.argument) {
                (None, None) => true,
                // `docs/signals.md` section 2.1: a set of signals is a set,
                // not a prefix. `signals("INT")` is a prefix of
                // `signals("INT,TERM")` as text and covers nothing of it.
                (Some(mine), Some(theirs)) if self.name == "signals" => {
                    signal_set_covers(mine, theirs)
                }
                // `docs/foreign-authority.md` section 4: the same for a set
                // of libraries. `ffi("libc")` is a prefix of `ffi("libcrypto")`
                // as text and covers nothing of it.
                (Some(mine), Some(theirs)) if self.name == "ffi" => scope_covers(mine, theirs),
                (Some(mine), Some(theirs))
                    if matches!(self.name.as_str(), "fs_read" | "fs_write" | "exec") =>
                {
                    extends_path(mine, theirs)
                }
                // `net_out` and `net_in` bound a `"host:port"` by plain text
                // (`docs/net.md` §4), which is also what `narrow` checks for
                // a `Net`; no label that carries an argument is left over.
                (Some(mine), Some(theirs)) => theirs.starts_with(mine.as_str()),
                _ => false,
            }
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.argument {
            Some(arg) => write!(f, "{}(\"{}\")", self.name, arg),
            None => write!(f, "{}", self.name),
        }
    }
}

impl Effects {
    pub fn pure() -> Self {
        Effects(Vec::new())
    }

    /// Canonicalise whatever was written: sorted, deduplicated.
    pub fn new(labels: impl IntoIterator<Item = Label>) -> Self {
        let mut labels: Vec<Label> = labels.into_iter().collect();
        labels.sort();
        labels.dedup();
        Effects(labels)
    }

    /// A row of plain labels, for the common case of no narrowing.
    pub fn plain(names: impl IntoIterator<Item = &'static str>) -> Self {
        Effects::new(names.into_iter().map(|n| Label { name: n.to_owned(), argument: None }))
    }

    pub fn labels(&self) -> &[Label] {
        &self.0
    }

    pub fn is_pure(&self) -> bool {
        self.0.is_empty()
    }

    /// One of the two operations §7.1 says are needed: union, to compute a
    /// body's effects.
    pub fn union(&mut self, other: &Effects) {
        self.0.extend(other.0.iter().cloned());
        self.0.sort();
        self.0.dedup();
    }

    /// Drop everything `authority` covers, for §8.2's discharge rule.
    ///
    /// Covering rather than equality: a function owning the capability a
    /// label narrows *from* has the authority for the narrowed label too,
    /// because narrowing is a call anyone holding one may make.
    pub fn discharge(&mut self, authority: &Effects) {
        self.0.retain(|label| !authority.0.iter().any(|held| held.covers(label)));
    }

    /// The other: subset, to check a call against a declaration. Linear in
    /// the number of labels, which is small and statically bounded.
    pub fn missing_from<'a>(&'a self, other: &Effects) -> Option<&'a Label> {
        self.0.iter().find(|l| !other.0.contains(l))
    }
}

impl fmt::Display for Effects {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let labels: Vec<String> = self.0.iter().map(Label::to_string).collect();
        write!(f, "[{}]", labels.join(", "))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    /// Truncating signed division. Division by zero and `int::MIN / -1` trap;
    /// neither is undefined behaviour.
    Div,
    /// Remainder, with the sign of the dividend. Traps on the same two inputs
    /// as `Div`.
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// `&&` and `||` short-circuit, so the backend lowers them as control flow
    /// rather than as an instruction.
    And,
    Or,
    /// The bit operators (`docs/bitwise.md`). `BitAnd` is `&` and is not
    /// `And`: this language has no truthy integer, so the two are never
    /// interchangeable and the checker says so rather than coercing.
    BitAnd,
    BitOr,
    BitXor,
    /// `<<` and `>>`. The amount traps outside `0..64` (§3), and neither
    /// traps on the value it produces (§4): a shift is bits, and bits do
    /// not overflow. `Shr` is arithmetic because `int` is signed (§2).
    Shl,
    Shr,
}

impl BinOp {
    pub fn is_comparison(self) -> bool {
        matches!(self, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge)
    }

    pub fn is_short_circuit(self) -> bool {
        matches!(self, BinOp::And | BinOp::Or)
    }

    /// The type this operator produces, given the type of its operands.
    pub fn result(self, operand: &Type) -> Type {
        if self.is_comparison() || self.is_short_circuit() { Type::Bool } else { operand.clone() }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Callee {
    Fn(FuncId),
    Builtin(Builtin),
    /// A foreign function (§8.4). Indexes [`Program::externs`].
    Extern(u32),
}

/// A foreign signature, as the backend needs it: the symbol to bind and the
/// shape of the call.
///
/// The capability parameters are still in `params` — the checker tracked a
/// real value through them — but they carry no data, so the backend drops
/// them on the way out to C exactly as it does for `putchar`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExternFn {
    pub name: String,
    /// The module that declares it (`docs/modules.md` §3).
    ///
    /// A foreign *symbol* is global to the linker, but a foreign *name*
    /// is a name like any other: `extern fn write` in one module and
    /// `pub fn write` in another are two functions, and only the first
    /// binds `write` in the object file. Without this, `std.buffer`
    /// owning a `write` made `extern fn write` unwritable anywhere in
    /// the program.
    pub module: u32,
    pub symbol: String,
    pub params: Vec<Type>,
    pub effects: Effects,
    pub ret: Type,
    /// `ret` was written `c_int` rather than `int` (`docs/reach.md` §3.4):
    /// the real C ABI return is a 32-bit `int`, not cancho's own 64-bit
    /// one, so a backend must cross it at that width and sign-extend --
    /// never read the full 64-bit return register, which the ABI never
    /// promised was clean above bit 31. Meaningless when `ret` is not
    /// `Type::Int` (`Bool` already crosses narrow at one byte; `Unit`
    /// crosses at none).
    pub narrow_return: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Expr {
    Int(i64),
    Bool(bool),
    Load(Slot),
    /// `base.index`, where `base` is a *reference* rather than a value.
    ///
    /// The same shape as [`Expr::Field`] and for the same reason — the
    /// backend owns where a field sits — except that it loads the field's
    /// leaves out of the buffer the reference points at instead of picking
    /// them out of leaves it already has.
    FieldRef {
        base: Box<Expr>,
        def: DefId,
        args: Vec<Type>,
        index: u32,
    },
    /// `base.index` where the field is `res`, so what comes back is a
    /// **reference to** the field rather than a copy of it
    /// (`docs/reading-references.md` §2.0).
    ///
    /// The same arithmetic as [`Expr::FieldRef`], stopping one step
    /// earlier: that node loads the field's leaves from `address +
    /// offset`, and this one is `address + offset`. A `res` field cannot
    /// be copied, so a borrow is the only thing reading one through a
    /// reference could mean.
    FieldAddr {
        base: Box<Expr>,
        def: DefId,
        args: Vec<Type>,
        index: u32,
    },
    /// `s[i]` — one element of a slice, bounds-checked (`defined-behaviour`
    /// §8). `element` is what comes back, which is how the backend knows
    /// the stride.
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        element: Type,
    },
    /// `s[a..b]` — a half-open run of a slice (`docs/slicing.md`).
    ///
    /// A slice is a pointer and a length in two registers, so this is
    /// `(ptr + a * stride, b - a)` after the bounds check: nothing is
    /// copied, nothing is allocated, and the result is the same two values
    /// a slice always was.
    Subslice {
        base: Box<Expr>,
        start: Box<Expr>,
        end: Box<Expr>,
        element: Type,
    },
    /// `fs_read(fs, path, into)` or `fs_write(fs, path, bytes)`.
    ///
    /// The prefix the capability was narrowed to travels with the node,
    /// because the backend emits the check against it and the type it came
    /// from is gone by then (`docs/filesystem.md` §4).
    FileOp {
        write: bool,
        prefix: String,
        args: Vec<Expr>,
    },
    /// `open_read(fs, path)` (`docs/file-handles.md` §2.1).
    ///
    /// Its own node for the same reason [`Expr::FileOp`] is one: the prefix
    /// the capability was narrowed to travels with it, because the backend
    /// emits the path check against that prefix and the type it came from
    /// is gone by then. What comes back is an `Opened`, tagged.
    OpenFile {
        prefix: String,
        mode: OpenMode,
        args: Vec<Expr>,
    },
    /// `exec_spawn(exec, path, args, env, stdin, stdout, stderr)`
    /// (`docs/processes.md` §3.2). Its own node for the reason
    /// [`Expr::OpenFile`] is one: the prefix travels with it, because the
    /// backend checks the path against it and the type it came from is gone
    /// by then. `args` is the capability (zero-sized) and the six arguments
    /// after it. What comes back is a `Spawned`, tagged. `in_dir` is
    /// `exec_spawn_in` (§4.10): `args[1]` is then a borrowed `Dir`, the
    /// directory the child starts in, and the path follows it.
    ExecSpawn {
        prefix: String,
        in_dir: bool,
        args: Vec<Expr>,
    },
    /// `tty_open(tty, path)` (`docs/tty.md` §3, edition 8). Its own node
    /// for the reason [`Expr::OpenFile`] is one: the prefix the capability
    /// was narrowed to travels with it, because the backend checks the
    /// path against it and the type it came from is gone by then. `args`
    /// is the capability (zero-sized) and the path. What comes back is a
    /// `TtyOpened`, tagged.
    TtyOpen {
        prefix: String,
        args: Vec<Expr>,
    },
    /// `fs_rename(fs, from, to)` and `fs_remove(fs, path)`
    /// (`docs/file-writes.md` section 7). Its own node for the reason
    /// [`Expr::OpenFile`] is one: the prefix travels with it, because the
    /// backend checks every path against it and the type it came from is gone
    /// by then. `args` is the capability (zero-sized) and then one path, or
    /// two for a rename. What comes back is a `Done`, tagged.
    PathOp {
        op: PathOp,
        prefix: String,
        args: Vec<Expr>,
    },
    /// `connect(net, name, port)` — `docs/net.md` §4.1
    /// (`docs/connect.md` §10): the name is checked against the bound at
    /// run time, then resolved with `getaddrinfo`.
    ///
    /// The bound the capability was narrowed to travels with the node for
    /// the same reason [`Expr::FileOp`]'s prefix does: the row it performs
    /// is this bound, and the type it came from is gone by lowering time.
    /// `args` is the capability (zero-sized, stopping at the backend), the
    /// name as `&r [byte]`, and the port as an `int`.
    Connect {
        bound: String,
        args: Vec<Expr>,
    },
    /// `bind(net, port)` — `docs/net.md` §2.1 (`docs/listen.md` §6): the
    /// inbound mirror of [`Expr::Connect`], bound by a port alone.
    ///
    /// `args` is the capability (zero-sized, stopping at the backend) and
    /// the port, an `int`.
    Bind {
        bound: String,
        args: Vec<Expr>,
    },
    /// `tcp_listen(net, port, backlog, flags)` --
    /// `docs/native-sockets.md` §3, edition 5: `bind` and `listen` answering
    /// a `Listening` instead of a descriptor. `args` is the capability
    /// (zero-sized), the port, the backlog and the flags, each an `int`.
    TcpListen {
        bound: String,
        args: Vec<Expr>,
        /// `udp_bind` (`docs/udp.md` §2): the same walk on a datagram socket, without `listen`,
        /// answering a `UdpOpened`.
        datagram: bool,
    },
    /// `tcp_connect(net, host, port)` -- `docs/native-sockets.md` §3,
    /// edition 5: `connect`'s check and walk answering a `Dialed`. `args` is
    /// the capability (zero-sized), the host as `&r [byte]`, and the port.
    TcpConnect {
        bound: String,
        args: Vec<Expr>,
        /// `udp_connect` (`docs/udp.md` §2): the same check and walk on a datagram socket,
        /// answering a `UdpOpened`. Never combined with `start`.
        datagram: bool,
        /// `tcp_connect_start`: the same walk with the socket made non-blocking first, so
        /// that `connect` answers `EINPROGRESS` instead of waiting (`docs/native-sockets.md` §10.6).
        start: bool,
    },
    /// A string literal's bytes (`docs/strings.md` §4). Lowered to a
    /// read-only data object plus the two leaves a slice is made of.
    Bytes(String),
    /// `len(s)` — a slice's length, which travels in the slice itself.
    Len(Box<Expr>),
    /// `alloc_slice[a](count, fill)` (§6): bump-allocate `count` elements
    /// and write `fill` into each.
    AllocSlice {
        arena: u32,
        element: Type,
        count: Box<Expr>,
        fill: Box<Expr>,
    },
    /// `alloc[a](value)` (§6): bump-allocate in arena `arena` and hand back
    /// a unique reference to what was written there.
    ///
    /// `ty` is what was allocated, which is how the backend knows how many
    /// bytes to take. It is always `val`: §6.1 refuses anything else,
    /// because an arena reclaims memory and runs nothing.
    Alloc {
        arena: u32,
        ty: Type,
        value: Box<Expr>,
    },
    /// `box(h, value)` (`docs/heap.md` §3): one `malloc`, and the value
    /// written into it.
    ///
    /// `ty` is what was boxed, which is how the backend knows how many bytes
    /// to ask for. Unlike an arena's `alloc` it may be `res`: a box's
    /// contents come back out through `unbox`, so an obligation put into one
    /// is an obligation that leaves again.
    Boxed {
        ty: Type,
        value: Box<Expr>,
    },
    /// `unbox(h, b)` (§3): read the value back, then one `free`.
    Unboxed {
        ty: Type,
        value: Box<Expr>,
    },
    /// `*r` — read what a reference points at
    /// (`docs/reading-references.md` §3).
    ///
    /// `ty` is what was read, which is how the backend knows how many
    /// leaves to load. It is always `val`: copying a `res` out of a
    /// reference would duplicate an obligation, which §4 exists to prevent.
    Deref {
        ty: Type,
        value: Box<Expr>,
    },
    /// `box_slice(h, count, fill)` (`docs/boxed-slices.md` §3): one
    /// `malloc`, then the fill written into every element -- or, when the
    /// fill is a constant zero, one `calloc` and no loop
    /// (`docs/zeroed-slices.md`, [`is_zero_fill`]).
    BoxedSlice {
        element: Type,
        count: Box<Expr>,
        fill: Box<Expr>,
    },
    /// `unbox_slice(h, b)` (§3): one `free`, and the element count back.
    UnboxedSlice {
        value: Box<Expr>,
    },
    /// `contents(b)` (§3): the dereference, which is one load — or two.
    ///
    /// A reference to a box is a pointer to where the box's own leaves
    /// live, so this reads them and *is* the reference to what the box
    /// holds. `ty` is what the box holds, which is how the backend knows
    /// how many leaves that is: one for an ordinary box, and two for a
    /// boxed *slice*, which carries a length as well
    /// (`docs/boxed-slices.md` §2).
    Contents {
        ty: Type,
        value: Box<Expr>,
    },
    /// A struct value. Fields are in *declaration* order whatever order they
    /// were written in, so the backend never has to consult a name.
    Struct {
        def: DefId,
        fields: Vec<Expr>,
    },
    /// `base.index`, by declaration position rather than by name. The struct
    /// is named too, so the backend never has to re-derive the base's type.
    Field {
        base: Box<Expr>,
        def: DefId,
        /// The base's type arguments, so the backend can compute the field's
        /// position without re-deriving the type.
        args: Vec<Type>,
        index: u32,
    },
    /// A tuple value (`docs/tuples.md`). Positional already, so unlike
    /// [`Expr::Struct`] there is no declaration order to reorder into.
    Tuple {
        parts: Vec<Expr>,
    },
    /// `base.index` on a tuple. The component types travel with the node
    /// for the same reason a struct's `args` do: the backend computes the
    /// leaf offset without re-deriving the base's type, and a tuple has no
    /// `DefId` to look one up with.
    TupleField {
        base: Box<Expr>,
        components: Vec<Type>,
        index: u32,
    },
    /// The same, where `base` is a *reference* — [`Expr::FieldRef`]'s
    /// counterpart for a type with no declaration.
    TupleFieldRef {
        base: Box<Expr>,
        components: Vec<Type>,
        index: u32,
    },
    /// The same, where the component is `res`: [`Expr::FieldAddr`]'s
    /// counterpart for a type with no declaration.
    TupleFieldAddr {
        base: Box<Expr>,
        components: Vec<Type>,
        index: u32,
    },
    /// An enum value: which variant, and its payload.
    Enum {
        def: DefId,
        args: Vec<Type>,
        variant: u32,
        payload: Vec<Expr>,
    },
    /// A floating-point constant, as bits (`docs/floating-point.md` §1).
    Float(u64),
    /// A binary32 constant, as bits (`docs/f32.md` §2).
    F32(u32),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    /// `~a` (`docs/bitwise.md` §1). Its own node rather than
    /// `Xor(a, -1)`, because the backend has the instruction and a reader
    /// of the IR should see what was written.
    BitNot(Box<Expr>),
    Bin {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Call {
        callee: Callee,
        args: Vec<Expr>,
    },
    /// A captureless function value, naming its target's own `DefId`
    /// (`docs/function-values.md` §4.2's "Identity" row): the address a
    /// call through it dials, and nothing else. The backend reads the
    /// target's own compiled signature off `Program::funcs`, the same
    /// place `Callee::Fn`'s own call already reads it from.
    FnValue(FuncId),
    /// A call through a value rather than a name: `h(args)` where `h`'s
    /// type is `Type::Fn` (`docs/function-values.md` §4.2's "A call
    /// through it performs exactly the row in its type").
    ///
    /// `params`/`ret` travel with the node because by lowering time only
    /// the *type* is known, the same reason [`Expr::FileOp`]'s prefix
    /// does — the backend needs them to build the indirect call's own
    /// signature, since there is no declaration to read one from.
    CallIndirect {
        target: Box<Expr>,
        args: Vec<Expr>,
        params: Vec<Type>,
        ret: Box<Type>,
    },
    /// `join(handle)` (`docs/threads.md` §2). Its own node rather than
    /// an ordinary `Expr::Call`, because `handle`'s type carries `R` --
    /// `join`'s real return type, which the backend needs to know how
    /// many leaves (zero, for `()`, or one) to read back out of what
    /// `pthread_join` wrote, and `Callee::Builtin` names no type of its
    /// own for it to read.
    Joined {
        handle: Box<Expr>,
        ret: Box<Type>,
    },
    /// A `static`'s data, by index into [`Program::statics`]
    /// (`docs/compile-time-data.md` §2).
    ///
    /// The same two leaves a string literal is — a pointer into read-only
    /// data and a length — because that is what it is. The difference is
    /// only in where the bytes came from: a literal's were written, and
    /// these were computed.
    Static(u32),
}

/// Where a value is written.
///
/// Two shapes, and deliberately not more: a whole local, or a field of
/// whatever a unique reference points at. Writing to a field of an *owned*
/// local would be a partial write, and what a partial write means for a
/// binding holding a `res` field is a question §4 does not answer — so it is
/// refused rather than guessed at.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Place {
    /// A whole local: `let`, `var`, a destructured part, or `x = e`.
    Slot(Slot),
    /// `r.field = e`, where `base` evaluates to a pointer. The same field
    /// arithmetic as [`Expr::FieldRef`], running the other way.
    Field { base: Expr, def: DefId, args: Vec<Type>, index: u32 },
    /// `s[i] = e`. `base` evaluates to a slice — a pointer and a length —
    /// and the write is bounds-checked exactly as a read is.
    Element { base: Expr, index: Expr, element: Type },
    /// `*r = e` (`docs/reading-references.md` §3). `base` evaluates to a
    /// unique reference and the whole referent is replaced.
    Deref { base: Expr, ty: Type },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Stmt {
    /// `let`/`var`, a destructured part, and assignment alike: by this point
    /// the difference is spent, having been checked during lowering.
    Store {
        place: Place,
        value: Expr,
    },
    /// An expression evaluated for its effects; its value is discarded.
    Eval(Expr),
    If {
        cond: Expr,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    Match {
        scrutinee: Expr,
        def: DefId,
        args: Vec<Type>,
        arms: Vec<Arm>,
        /// Was the scrutinee a *reference* to the enum rather than the enum
        /// (`docs/reading-references.md` §2)?
        ///
        /// It changes what the arms bind — references into the referent
        /// rather than the payload itself — and it changes what the whole
        /// statement costs: nothing is consumed, because nothing was owned.
        by_reference: bool,
    },
    /// `region a { .. }` (§6).
    ///
    /// `arena` numbers this function's arenas in the order they open, which
    /// is what an `Expr::Alloc` inside the body names. The region itself is
    /// a block in the same table `borrow` uses -- an arena's lifetime and a
    /// borrow's lifetime are one mechanism, which is §6's claim.
    Region {
        arena: u32,
        body: Vec<Stmt>,
    },
    /// `borrow x as &r in { .. }` (§5).
    ///
    /// `referent` is spilled to a buffer for the duration and `reference`
    /// holds a pointer at it. The referent is frozen for the whole block, so
    /// nothing can change underneath the pointer and nothing has to be
    /// written back when the block closes.
    Borrow {
        referent: Slot,
        reference: Slot,
        /// A unique borrow writes the buffer back into the referent when the
        /// block closes; a shared one has nothing to write back, because the
        /// referent was frozen and cannot have drifted.
        unique: bool,
        body: Vec<Stmt>,
    },
    Return(Expr),
}

/// One arm of a `match`, with its pattern already resolved to a variant index
/// and its bindings to slots.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Arm {
    /// `None` is the wildcard arm.
    pub variant: Option<u32>,
    /// One per payload position; `None` where the pattern wrote `_`.
    pub bindings: Vec<Option<Slot>>,
    pub body: Vec<Stmt>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Func {
    pub name: String,
    /// The module that declares it, as a dotted path (`std.json`), empty for
    /// the root module (`docs/modules.md` §3).
    ///
    /// A name is only unique *within* a module, so this is half of what
    /// identifies a function to a linker: two modules may each declare a
    /// `pub fn drop`, and `std.buffer` and `std.vec` and `std.json` do.
    /// Before this field the backends named every function `lexs_<name>`,
    /// the type checker accepted the program, and the *assembler* refused
    /// it with `invalid redefinition of function 'lexs_drop'`.
    pub module: String,
    /// The declared row, checked exact against what the body performs.
    pub effects: Effects,
    /// What the body actually performs, **before** ownership discharges
    /// it (`docs/authority.md` §2).
    ///
    /// The same set for every function that borrows its authority, and
    /// different for exactly the one that does not: `main` owns what
    /// `split` gave it, so its declared row is `[]` however much it does.
    /// A program's authority surface is the union of *these*, and reading
    /// the declared rows instead silently loses whatever `main` did
    /// itself.
    pub performs: Effects,
    pub n_params: u32,
    /// One entry per slot, parameters first. The backend reads these to pick a
    /// machine type, so every slot's type is resolved before it gets here.
    pub slots: Vec<Type>,
    pub ret: Type,
    pub body: Vec<Stmt>,
    /// How many operators in this body were evaluated during lowering
    /// (`docs/compile-time.md` §2.1).
    ///
    /// Reported rather than kept quiet: a compiler that rewrites a
    /// program's own arithmetic should be able to say how much of it it
    /// rewrote, and §9 lists the alternative — silent folding — as the
    /// thing that makes the pass hard to trust.
    pub folded: usize,
    /// Where the function is declared, and the only span the IR carries
    /// (`docs/internal-errors.md` §2): it is what a backend failure in
    /// this function points at. A generic instance carries its generic
    /// declaration's. Nothing finer, for `compile-time.md` §9's reason.
    pub span: Span,
}

impl Func {
    /// The name a backend gives the function in the object file, without
    /// the `lexs_` every backend prefixes: the bare name for a root-module
    /// function (so `main` stays `main`), and `module.name` for any other --
    /// `.` is legal in an object-file symbol, and cannot appear in a
    /// cancho identifier, so the two cannot collide.
    pub fn symbol(&self) -> String {
        if self.module.is_empty() {
            self.name.clone()
        } else {
            format!("{}.{}", self.module, self.name)
        }
    }

    pub fn n_slots(&self) -> u32 {
        self.slots.len() as u32
    }

    /// **Provably pure**: calling it twice with the same arguments gives
    /// the same answer and changes nothing a caller can see.
    ///
    /// Two conditions, and `docs/purity.md` §2 is why they are the whole
    /// list:
    ///
    /// 1. `performs` is empty — no console, filesystem, foreign call,
    ///    heap or command line. The *declared* row is the wrong one to
    ///    read here: `main`'s is `[]` however much it does, because
    ///    owning discharges (`authority.md` §2).
    /// 2. No parameter reaches a **unique** reference. A row of `[]` does
    ///    not mean pure on its own — `std.vec`'s `set` declares `[]` and
    ///    writes through `&!v Vec[T]`, which is exactly the case this
    ///    second condition exists for.
    ///
    /// Nothing else can leak: `alloc` only works inside a `region r { .. }`
    /// block that is lexically open, so a function cannot allocate into a
    /// caller's arena, and an owned `res` argument cannot be supplied
    /// twice because linearity forbids it.
    ///
    /// **This is not "safe to hoist".** A pure function may still trap,
    /// and moving a trap out of a loop that runs zero times invents one.
    /// Purity licenses common-subexpression elimination where the call
    /// already happens; speculative motion needs "does not trap" as well,
    /// which this does not claim (`docs/purity.md` §3).
    pub fn is_pure(&self) -> bool {
        fn writes_through(ty: &Type) -> bool {
            match ty {
                Type::Ref { unique, inner, .. } => *unique || writes_through(inner),
                Type::Slice(element) => writes_through(element),
                Type::Tuple(parts) => parts.iter().any(writes_through),
                Type::Named(_, args) => args.iter().any(writes_through),
                _ => false,
            }
        }

        self.performs.is_pure() && !self.slots[..self.n_params as usize].iter().any(writes_through)
    }
}

/// A declared type, as the backend needs it: names for diagnostics and member
/// types in declaration order.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TypeInfo {
    Struct { name: String, fields: Vec<(String, Type)> },
    Enum { name: String, variants: Vec<(String, Vec<Type>)> },
}

impl TypeInfo {
    pub fn name(&self) -> &str {
        match self {
            TypeInfo::Struct { name, .. } | TypeInfo::Enum { name, .. } => name,
        }
    }
}

/// A `static`'s evaluated contents (`docs/compile-time-data.md` §2).
///
/// Scalars rather than bytes, so the backend lays them out with the same
/// `stride` it uses for every other slice of this element type — which is
/// how a `[byte]` static comes out one byte per element and an `[int]`
/// one comes out eight, without this crate knowing either number.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StaticValue {
    pub name: String,
    pub element: Type,
    /// One entry per element. A `float` is its bits, which is the same
    /// thing `Expr::Float` holds and for the same reason (`f64` is not
    /// `Eq`).
    pub values: Vec<i64>,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Program {
    pub funcs: Vec<Func>,
    /// Every `static`, in declaration order. A `static` may read one
    /// declared before it and not one declared after, which is the
    /// cheapest rule that has no cycles in it.
    pub statics: Vec<StaticValue>,
    /// How many calls were evaluated at compile time
    /// (`docs/compile-time.md` §3), across the whole program, and how
    /// many operators the call pass exposed on top of the ones lowering
    /// had already folded.
    pub folded_calls: usize,
    pub folded_late: usize,
    /// Foreign functions the unit declared, in declaration order.
    pub externs: Vec<ExternFn>,
    /// Indexed by [`DefId`].
    pub types: Vec<TypeInfo>,
}

impl Program {
    pub fn func(&self, id: FuncId) -> &Func {
        &self.funcs[id.0 as usize]
    }

    pub fn type_info(&self, def: DefId) -> &TypeInfo {
        &self.types[def.0 as usize]
    }

    /// The `World` type, for a driver that needs to check `main`'s shape.
    pub fn world(&self) -> Type {
        Type::Named(DefId(PRELUDE_WORLD as u32), Vec::new())
    }

    pub fn find(&self, name: &str) -> Option<FuncId> {
        self.funcs.iter().position(|f| f.name == name).map(|i| FuncId(i as u32))
    }
}

/// Does every path through these statements end in a `return`?
///
/// Deliberately structural and therefore conservative: a `while` never counts,
/// even `while true { }`. A conservative answer is a total one, and a program
/// can always say `return` once more.
///
/// The backend asks the same question, so the two agree by construction.
pub fn terminates(body: &[Stmt]) -> bool {
    match body.last() {
        Some(Stmt::Return(_)) => true,
        Some(Stmt::If { then_body, else_body, .. }) => {
            !else_body.is_empty() && terminates(then_body) && terminates(else_body)
        }
        // A `match` reaching here is exhaustive -- the checker refuses any
        // other kind -- so if every arm returns, so does the match.
        Some(Stmt::Match { arms, .. }) => arms.iter().all(|arm| terminates(&arm.body)),
        // A `borrow` block runs unconditionally, exactly once, so it
        // terminates when its body does. Unlike a `while`, there is no
        // question of whether it is entered.
        Some(Stmt::Borrow { body, .. } | Stmt::Region { body, .. }) => terminates(body),
        _ => false,
    }
}

/// Whether a `box_slice` fill is a constant whose every bit is zero: `0`,
/// `false`, `0.0`, or `byte_of(0)`. Such a slice can be had from `calloc`
/// without a fill loop -- the allocator hands back zeroed memory, and for
/// a large one fresh pages the kernel zeroes only when they are first
/// touched (`docs/zeroed-slices.md`). Anything else, including a `0`
/// computed at run time, keeps the loop: this is a decision about the
/// program's text, so both backends make it the same way.
pub fn is_zero_fill(fill: &Expr) -> bool {
    match fill {
        Expr::Int(0) | Expr::Bool(false) | Expr::Float(0) | Expr::F32(0) => true,
        Expr::Call { callee: Callee::Builtin(Builtin::ByteOf), args } => {
            matches!(args.as_slice(), [Expr::Int(0)])
        }
        _ => false,
    }
}

/// The `open` flags the file and directory builtins pass, per target
/// (`docs/directory-handles.md` §2 and §3). `O_RDONLY` is zero on Linux and
/// Darwin and **not on WASI**, where it is `0x04000000` and a zero access
/// mode asks for no rights at all (`EINVAL`), so a read-only open ORs in
/// `read_only`. Written once here so both backends spell them the same; the
/// values are the kernels' own (WASI's are wasi-libc's), and Linux x86-64
/// and AArch64 differ only in the first two.
///
/// `cloexec` is on every open (`docs/processes.md` §4.5): no descriptor a
/// builtin opens crosses an `exec`. Every open is an `openat`, from
/// `at_fdcwd` when it names a path rather than a name beneath a `Dir`, so
/// the flag is set by the call that makes the descriptor, never after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenFlags {
    /// `O_RDONLY`: zero except on WASI.
    pub read_only: i64,
    /// `O_RDWR`: 2 on Linux and Darwin, but on WASI it is `O_RDONLY | O_WRONLY`
    /// (`0x14000000`), because there neither access mode is zero.
    pub read_write: i64,
    pub directory: i64,
    pub nofollow: i64,
    pub write_only: i64,
    pub create: i64,
    pub exclusive: i64,
    pub append: i64,
    pub truncate: i64,
    pub cloexec: i64,
    /// `AT_FDCWD`: `openat`'s "relative to the working directory".
    pub at_fdcwd: i64,
}

/// The operating system a target runs on, as far as the constants in this
/// file are concerned. Three, and a `match` on it names all three, so a
/// fourth is a compile error at every site that has to know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Linux,
    Darwin,
    /// WASI preview 1 (`wasm32-wasip1`), through wasi-libc (`docs/wasm.md`).
    Wasi,
}

/// [`open_flags_for`] for the two hosts Cranelift can target.
pub fn open_flags(darwin: bool, aarch64: bool) -> OpenFlags {
    open_flags_for(if darwin { Os::Darwin } else { Os::Linux }, aarch64)
}

pub fn open_flags_for(os: Os, aarch64: bool) -> OpenFlags {
    if os == Os::Wasi {
        // wasi-libc's `__header_fcntl.h`. `O_CLOEXEC` is 0 there: a WASI
        // module cannot `exec`, so there is nothing for the flag to protect.
        // `AT_FDCWD` is -2, as on Darwin.
        return OpenFlags {
            read_only: 0x0400_0000,
            read_write: 0x1400_0000,
            directory: 0x2000,
            nofollow: 0x0100_0000,
            write_only: 0x1000_0000,
            create: 0x1000,
            exclusive: 0x4000,
            append: 0x0001,
            truncate: 0x8000,
            cloexec: 0,
            at_fdcwd: -2,
        };
    }
    if os == Os::Darwin {
        OpenFlags {
            read_only: 0,
            read_write: 2,
            directory: 0x0010_0000,
            nofollow: 0x0100,
            write_only: 1,
            create: 0x0200,
            exclusive: 0x0800,
            append: 0x0008,
            truncate: 0x0400,
            cloexec: 0x0100_0000,
            at_fdcwd: -2,
        }
    } else if aarch64 {
        OpenFlags {
            read_only: 0,
            read_write: 2,
            directory: 0o40000,
            nofollow: 0o100000,
            write_only: 1,
            create: 0o100,
            exclusive: 0o200,
            append: 0o2000,
            truncate: 0o1000,
            cloexec: 0o2000000,
            at_fdcwd: -100,
        }
    } else {
        OpenFlags {
            read_only: 0,
            read_write: 2,
            directory: 0o200000,
            nofollow: 0o400000,
            write_only: 1,
            create: 0o100,
            exclusive: 0o200,
            append: 0o2000,
            truncate: 0o1000,
            cloexec: 0o2000000,
            at_fdcwd: -100,
        }
    }
}

/// The mode a file created beneath a directory gets: `0644`, as `creat` and
/// `fopen` give everywhere else in this compiler.
pub const CREATE_MODE: i64 = 0o644;

/// The longest name `dir_enter` and `dir_open_read` copy (`NAME_MAX`, 255 on
/// Linux and Darwin); a longer one is `EINVAL` too, so the copy has a fixed
/// size.
pub const NAME_MAX: i64 = 255;

/// Where `readdir`'s `struct dirent` keeps the two fields a listing reads
/// (`docs/directory-listing.md` §3.4). Linux x86-64 measured with `offsetof`;
/// Linux AArch64 has glibc's same generic layout; Darwin's is its 64-bit-inode
/// `dirent` (`d_seekoff` and `d_namlen` push both two bytes on). `d_name` is
/// NUL-terminated on every target, so its length is `strlen`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirentLayout {
    pub d_type: i32,
    pub d_name: i32,
}

pub fn dirent_layout(darwin: bool) -> DirentLayout {
    dirent_layout_for(if darwin { Os::Darwin } else { Os::Linux })
}

/// wasi-libc's `struct dirent` is `{ ino_t d_ino; unsigned char d_type;
/// char d_name[]; }`, so `d_type` is at 8 and `d_name` at 9.
pub fn dirent_layout_for(os: Os) -> DirentLayout {
    match os {
        Os::Darwin => DirentLayout { d_type: 20, d_name: 21 },
        Os::Linux => DirentLayout { d_type: 18, d_name: 19 },
        Os::Wasi => DirentLayout { d_type: 8, d_name: 9 },
    }
}

/// `ENAMETOOLONG`: what `dir_next` answers when the caller's buffer is
/// shorter than the name.
pub fn enametoolong(darwin: bool) -> i64 {
    enametoolong_for(if darwin { Os::Darwin } else { Os::Linux })
}

pub fn enametoolong_for(os: Os) -> i64 {
    match os {
        Os::Darwin => 63,
        Os::Linux => 36,
        // The language's number, not WASI's own 37: `errno` is translated on
        // WASI (`errno.rs`), so this is what a failing call would have said.
        Os::Wasi => 36,
    }
}

/// `d_type`'s values on Linux and Darwin, and the language's own numbering
/// of a kind (`docs/directory-listing.md` §3.1). **Not the same on every
/// target**: WASI's are different numbers (`DT_DIR` is 3, `DT_REG` 4, `DT_LNK`
/// 7), so a backend that can target it asks [`dirent_types`].
pub const DT_DIR: i64 = 4;
pub const DT_REG: i64 = 8;
pub const DT_LNK: i64 = 10;
pub const DT_UNKNOWN: i64 = 0;
/// The four `d_type` values a listing translates, for one target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirentTypes {
    pub unknown: i64,
    pub link: i64,
    pub dir: i64,
    pub reg: i64,
}

pub fn dirent_types(os: Os) -> DirentTypes {
    match os {
        Os::Linux | Os::Darwin => {
            DirentTypes { unknown: DT_UNKNOWN, link: DT_LNK, dir: DT_DIR, reg: DT_REG }
        }
        // wasi-libc's `__header_dirent.h`.
        Os::Wasi => DirentTypes { unknown: 0, link: 7, dir: 3, reg: 4 },
    }
}
pub const KIND_UNKNOWN: i64 = 0;
pub const KIND_FILE: i64 = 1;
pub const KIND_DIRECTORY: i64 = 2;
pub const KIND_LINK: i64 = 3;
pub const KIND_OTHER: i64 = 4;

/// Where `struct stat` keeps what `dir_stat` reads (`docs/directory-listing.md`
/// §3.4): its size (the stack slot `fstatat` fills), `st_mode` and how wide it
/// is, `st_size`, and `st_mtim`'s seconds. Linux x86-64 measured with
/// `offsetof`; Linux AArch64 is the generic layout; Darwin's is the 64-bit-inode
/// `stat`, whose `st_mode` is 16 bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatLayout {
    pub size: u32,
    pub mode: i32,
    pub mode_bits: u8,
    pub st_size: i32,
    pub mtime: i32,
    /// `AT_SYMLINK_NOFOLLOW` for this target.
    pub no_follow: i64,
}

pub fn stat_layout(darwin: bool, aarch64: bool) -> StatLayout {
    stat_layout_for(if darwin { Os::Darwin } else { Os::Linux }, aarch64)
}

/// wasi-libc's `struct stat` (`__struct_stat.h`) has Linux x86-64's offsets
/// -- `st_mode` at 24, `st_size` at 48, `st_mtim` at 88, 144 bytes -- but its
/// own `AT_SYMLINK_NOFOLLOW`, `0x1`.
pub fn stat_layout_for(os: Os, aarch64: bool) -> StatLayout {
    match (os, aarch64) {
        (Os::Darwin, _) => StatLayout {
            size: 144,
            mode: 4,
            mode_bits: 16,
            st_size: 96,
            mtime: 48,
            no_follow: 0x20,
        },
        (Os::Wasi, _) => StatLayout {
            size: 144,
            mode: 24,
            mode_bits: 32,
            st_size: 48,
            mtime: 88,
            no_follow: 0x1,
        },
        (Os::Linux, true) => StatLayout {
            size: 128,
            mode: 16,
            mode_bits: 32,
            st_size: 48,
            mtime: 88,
            no_follow: 0x100,
        },
        (Os::Linux, false) => StatLayout {
            size: 144,
            mode: 24,
            mode_bits: 32,
            st_size: 48,
            mtime: 88,
            no_follow: 0x100,
        },
    }
}

/// What `dir_rename_new` answers when the filesystem cannot refuse to
/// replace: `EOPNOTSUPP` on Linux, `ENOTSUP` on Darwin. The kernel says
/// `EINVAL` for an unknown flag, and `EINVAL` is also what a name that is not
/// one component gets, so it is mapped here to a value a program can tell
/// apart (`docs/directory-handles.md` §3, slice 4).
pub fn rename_unsupported(darwin: bool) -> i64 {
    if darwin { 45 } else { 95 }
}

/// `renameatx_np`'s `RENAME_EXCL` and Linux's `RENAME_NOREPLACE`, the flag
/// each target's no-replace rename takes.
pub fn rename_no_replace(darwin: bool) -> i64 {
    if darwin { 0x4 } else { 0x1 }
}

/// `st_mode`'s permission bits (set-user-id, set-group-id, sticky, and the
/// nine read/write/execute bits), what `dir_mode` answers
/// (`docs/directory-listing.md` §3.5). The same on every target.
pub const PERMISSION_BITS: i64 = 0o7777;

/// `st_mode`'s file-type bits, the same on every target.
pub const S_IFMT: i64 = 0o170000;
pub const S_IFREG: i64 = 0o100000;
pub const S_IFDIR: i64 = 0o040000;
pub const S_IFLNK: i64 = 0o120000;
