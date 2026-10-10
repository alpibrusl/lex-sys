//! Declarations: signatures, monomorphic instances, type definitions,
//! the prelude's types and imports, and what a type's capabilities
//! discharge.

use crate::*;

/// A function's signature: what a caller is checked against, and all a caller
/// is ever checked against.
pub(crate) struct Signature {
    /// The declared effect row (§7.2): written at the boundary, never
    /// inferred across one. A caller is checked against this and never
    /// against the body that justifies it.
    pub(crate) effects: Effects,
    /// Region parameters, in declaration order; `Region::Param(i)` is the
    /// `i`th. Not part of monomorphisation: see `lower_function`.
    pub(crate) regions: Vec<Symbol>,
    /// `where a <= b` as indices into `regions`, meaning `b` outlives `a`.
    pub(crate) outlives: Vec<(u32, u32)>,
    pub(crate) name: Symbol,
    /// A `val` bound per type parameter (`docs/mode-polymorphism.md`
    /// §3.1). Part of the signature: it is what a caller is checked
    /// against, so it is also what a caller depends on.
    pub(crate) bounds: Vec<Option<Mode>>,
    /// The module that declares it (`docs/modules.md` §3).
    pub(crate) module: u32,
    /// `pub` — callable from another module (§5).
    pub(crate) public: bool,
    /// Type parameters in declaration order; `Type::Param(i)` is the `i`th.
    pub(crate) generics: Vec<Symbol>,
    pub(crate) params: Vec<Type>,
    pub(crate) ret: Type,
    /// Position of the declaration among the unit's items.
    pub(crate) item: usize,
}

/// One monomorphic copy of a function: which function, and the type arguments
/// it was instantiated at. A non-generic function has exactly one, with no
/// arguments.
pub(crate) struct Instance {
    pub(crate) signature: usize,
    pub(crate) args: Vec<Type>,
}

/// The monomorphisation worklist.
///
/// Generics are erased by *copying*: `first[int]` and `first[bool]` become two
/// ordinary functions. That is the commitment in #1 — monomorphised generics,
/// zero cost — and it is what lets the backend keep scalarising, since every
/// function it sees has concrete types.
///
/// A copy's id is its index here, assigned when a call site first asks for it.
/// The body may not be lowered yet, which is why ids are handed out eagerly
/// and the bodies filled in afterwards: a generic function may call itself.
pub(crate) struct Mono {
    pub(crate) instances: Vec<Instance>,
    pub(crate) pending: Vec<usize>,
    /// False while checking a generic function rigidly, where instantiations
    /// are hypothetical and must not be emitted.
    pub(crate) recording: bool,
}

impl Mono {
    pub(crate) fn new(recording: bool) -> Self {
        Self { instances: Vec::new(), pending: Vec::new(), recording }
    }

    /// The id of this instance, creating it if it is new.
    pub(crate) fn request(&mut self, signature: usize, args: Vec<Type>) -> FuncId {
        if !self.recording {
            // Checking a generic body: the call is type-checked, but no copy
            // is emitted for a type argument that is itself a parameter.
            return FuncId(0);
        }
        if let Some(index) =
            self.instances.iter().position(|i| i.signature == signature && i.args == args)
        {
            return FuncId(index as u32);
        }
        self.instances.push(Instance { signature, args });
        let index = self.instances.len() - 1;
        self.pending.push(index);
        FuncId(index as u32)
    }
}

/// A name for one monomorphic copy.
///
/// `first[int, bool]` becomes `first$int$bool`. Two instantiations of the same
/// function must not collide, and the name is what the linker sees.
pub(crate) fn instance_name(base: &str, args: &[Type], unifier: &Unifier) -> String {
    if args.is_empty() {
        return base.to_owned();
    }
    let mut name = base.to_owned();
    for arg in args {
        name.push('$');
        name.push_str(&unifier.display(arg).replace(['[', ']', ' '], "_").replace(',', "_"));
    }
    name
}

/// A declared type, as the checker needs it: interned names, so a member
/// lookup is an integer comparison.
pub(crate) enum DefKind {
    Struct(Vec<(Symbol, Type)>),
    Enum(Vec<(Symbol, Vec<Type>)>),
}

/// The module the prelude's types belong to.
///
/// Not a real module: `World`, `Io`, `Box` and the rest are the
/// *language*, not a library, so they are visible from every module and
/// there is nothing to import. `docs/modules.md` §3 says a file that
/// declares nothing is in the root; this is the one thing that is in no
/// module at all.
pub(crate) const PRELUDE_MODULE: u32 = u32::MAX;

pub(crate) struct TypeDef {
    pub(crate) name: Symbol,
    pub(crate) def: DefId,
    /// The module that declares it (`docs/modules.md` §3), or
    /// [`PRELUDE_MODULE`]. Two modules may declare one name; this is what
    /// tells them apart.
    pub(crate) module: u32,
    /// `pub` — reachable from another module (§5).
    pub(crate) public: bool,
    /// Type parameters in declaration order; `Type::Param(i)` is the `i`th.
    pub(crate) generics: Vec<Symbol>,
    /// A `val` bound per parameter, parallel to `generics`
    /// (`docs/collections.md` §3). Kept where the type argument is
    /// supplied, which is where the argument is known.
    pub(crate) bounds: Vec<Option<Mode>>,
    /// The mode the declaration wrote, if it wrote one. `None` means the mode
    /// is whatever the members make it (§3).
    pub(crate) declared_mode: Option<Mode>,
    pub(crate) kind: DefKind,
    pub(crate) span: Span,
    /// The edition a file must be at to name this declaration
    /// (`docs/editions.md` §7). `1` for everything a program could
    /// always name; the prelude's own edition-2 additions are the
    /// first thing to write anything else here. A user declaration
    /// carries the edition of the file that wrote it.
    pub(crate) since: u32,
}

impl TypeDef {
    /// The bound on the `i`th parameter, counting what the declaration's own
    /// mode implies (`docs/collections.md` §3).
    ///
    /// A `val` aggregate is `val` at *every* instantiation, which is only
    /// true if every argument is -- so saying `val` bounds them all, and the
    /// parser refuses writing it a second time. Anything else carries only
    /// what it wrote.
    pub(crate) fn bound(&self, i: usize) -> Option<Mode> {
        if self.declared_mode == Some(Mode::Val) {
            return Some(Mode::Val);
        }
        self.bounds.get(i).copied().flatten()
    }

    /// Is this declaration a candidate for a name looked up in `module`?
    ///
    /// Its own module, or the prelude — which is in no module and
    /// reachable from all of them.
    pub(crate) fn visible_from(&self, module: u32) -> bool {
        self.module == module || self.module == PRELUDE_MODULE
    }

    /// Every type this one holds directly, for the size check and for the
    /// structural mode computation.
    pub(crate) fn members(&self) -> Box<dyn Iterator<Item = &Type> + '_> {
        match &self.kind {
            DefKind::Struct(fields) => Box::new(fields.iter().map(|(_, ty)| ty)),
            DefKind::Enum(variants) => Box::new(variants.iter().flat_map(|(_, p)| p.iter())),
        }
    }
}

/// Can `from` reach `target` by following member types?
///
/// A type that contains itself — directly or through others — has no finite
/// size. There is no representation to pick and no depth to stop at, so it
/// is refused rather than approximated. That covers
/// `enum List { Nil, Cons(int, List) }` as much as a self-referential struct.
///
/// **Except through a `Box`** (`docs/heap.md` §4). A box is a pointer, so it
/// is one leaf however large what it points at is, and the size computation
/// terminates. That hole is the whole reason the heap exists: every linked
/// structure in computing is a self-referential declaration plus exactly
/// this indirection.
pub(crate) fn reaches(defs: &[TypeDef], from: usize, target: usize, seen: &mut [bool]) -> bool {
    if seen[from] {
        return false;
    }
    seen[from] = true;
    defs[from].members().any(|ty| reaches_through(defs, ty, target, seen))
}

/// The same walk, over one member rather than a definition's whole list.
///
/// Split out because a member's *type arguments* have to be followed too.
/// They were not before this existed, so `struct Node { w: Wrap[Node] }` was
/// accepted although it has no more finite a size than `Wrap` written out:
/// no program could build one, because the checker refused every attempt at
/// a value, but the refusal landed at each use rather than at the
/// declaration that was wrong.
pub(crate) fn reaches_through(
    defs: &[TypeDef],
    ty: &Type,
    target: usize,
    seen: &mut [bool],
) -> bool {
    // A tuple is an aggregate with no declaration, so it is transparent to
    // this walk: `struct Node { pair: (int, Node) }` contains itself just as
    // surely as `struct Node { next: Node }` does, and is refused for the
    // same reason (`docs/tuples.md` §2).
    if let Type::Tuple(parts) = ty {
        return parts.iter().any(|part| reaches_through(defs, part, target, seen));
    }
    let Type::Named(def, args) = ty else {
        return false;
    };
    let next = def.0 as usize;
    if next == PRELUDE_BOX {
        return false;
    }
    next == target
        || reaches(defs, next, target, seen)
        || args.iter().any(|arg| reaches_through(defs, arg, target, seen))
}

/// Does a value of this type occupy no machine values at all?
///
/// §8.1: "capabilities erase at compile time except where they carry data".
/// A struct with no fields has no leaves, so threading one is free -- which
/// is a claim worth a test rather than a comment.
pub fn leaf_free(ty: &Type) -> bool {
    matches!(ty, Type::Named(def, _)
    if matches!(
        def.0 as usize,
        PRELUDE_WORLD
            | PRELUDE_IO
            | PRELUDE_FFI
            | PRELUDE_FS
            | PRELUDE_HEAP
            | PRELUDE_ARGS
            | PRELUDE_NET
            | PRELUDE_CLOCK
            | PRELUDE_SIGNALS
            | PRELUDE_EXEC
    ))
}

/// Is this the file handle (`docs/file-handles.md`)?
///
/// Unlike the six capabilities above it is **not** leaf-free: a descriptor
/// is a number the kernel gave us, so a `File` is one leaf. That is the
/// whole difference between it and an `Io`, which is authority with nothing
/// behind it.
pub fn is_file(def: DefId) -> bool {
    def.0 as usize == PRELUDE_FILE
}

/// Is this one of the prelude's capability types?
///
/// §8.2: authority comes from exactly one place, and a program that could
/// *write* `Io { }` would have an ambient constructor by another name — the
/// very thing "no `Io::global()`, no `unsafe { }` that conjures one" rules
/// out. So a capability has no literal form, and the only `Io` in existence
/// is the one the runtime handed to `main` inside a `World`.
pub(crate) fn is_capability(def: DefId) -> bool {
    matches!(
        def.0 as usize,
        PRELUDE_WORLD
            | PRELUDE_IO
            | PRELUDE_FFI
            | PRELUDE_FS
            | PRELUDE_HEAP
            | PRELUDE_ARGS
            | PRELUDE_SPLIT
            | PRELUDE_SPLIT_NET
            | PRELUDE_SPLIT_CLOCK
            | PRELUDE_SPLIT_SIGNALS
            | PRELUDE_SPLIT_EXEC
            | PRELUDE_EXEC
            | PRELUDE_CHILD
            | PRELUDE_PIPE
            | PRELUDE_CHILD_END
            | PRELUDE_CLOCK
            | PRELUDE_SIGNALS
            | PRELUDE_SIGNAL_WATCH
            | PRELUDE_NET
            | PRELUDE_TTY
            | PRELUDE_SPLIT_TTY
            // §8.2 again, and more sharply: a program that could write
            // `File { }` would be conjuring a descriptor, which is worse
            // than conjuring authority because the number would be someone
            // else's open file.
            | PRELUDE_FILE
            | PRELUDE_DIR
            | PRELUDE_DIR_LIST
            | PRELUDE_LISTENER
            | PRELUDE_CONN
            | PRELUDE_UDP
            | PRELUDE_POLLER
    )
}

/// Is this a capability whose only consumer is `release`?
///
/// `Split` is not: taking it apart is the whole point of having one.
/// `World` and `Io` are, because destructuring either would destroy
/// authority without naming the function that knows how (§4.1) — and for
/// `World` and `Io`, which carry no fields, it would do it silently.
pub(crate) fn released_only(def: DefId) -> bool {
    matches!(
        def.0 as usize,
        PRELUDE_WORLD
            | PRELUDE_IO
            | PRELUDE_FFI
            | PRELUDE_FS
            | PRELUDE_HEAP
            | PRELUDE_ARGS
            | PRELUDE_NET
            | PRELUDE_CLOCK
            | PRELUDE_SIGNALS
            | PRELUDE_EXEC
    )
}

/// Is this a type whose only consumer is `close` (`docs/file-handles.md`)?
///
/// The same rule as [`released_only`] and [`unboxed_only`], pointed at the
/// third thing that owns something the language cannot see: a descriptor.
/// Destructuring a `File` would drop it without calling `close`, which is
/// a leak the kernel keeps rather than one the allocator does.
pub(crate) fn closed_only(def: DefId) -> bool {
    matches!(
        def.0 as usize,
        PRELUDE_FILE
            | PRELUDE_DIR
            | PRELUDE_DIR_LIST
            | PRELUDE_LISTENER
            | PRELUDE_CONN
            | PRELUDE_UDP
            | PRELUDE_TTY
            | PRELUDE_POLLER
            | PRELUDE_SIGNAL_WATCH
            // `docs/processes.md` §4.7: a child is ended by `child_wait`, a
            // channel's end by `pipe_close`, and the child's end by
            // `exec_spawn` or `child_end_close` -- never by a pattern.
            | PRELUDE_CHILD
            | PRELUDE_PIPE
            | PRELUDE_CHILD_END
    )
}

/// Is this a type whose only consumer is `unbox` (`docs/heap.md` §3)?
///
/// `Box[T]` carries no *fields* — what it owns is an allocation, and the
/// pointer to it is not something a pattern can name. Destructuring one
/// would end the allocation without naming the function that frees it,
/// which is §4.1's rule and the same reason a capability may not be taken
/// apart.
pub(crate) fn unboxed_only(def: DefId) -> bool {
    def.0 as usize == PRELUDE_BOX
}

/// Does `target` name something *inside* `prefix` (`docs/filesystem.md` §1)?
///
/// A path prefix is not a byte prefix. `/tmp` contains `/tmp/a`, and it does
/// not contain `/tmpevil` — the two share five bytes and nothing else. So an
/// extension has to land on a separator, at compile time when `narrow` is
/// checked and again at run time when a path is handed to an operation.
///
/// The empty prefix contains everything, which is what makes the capability
/// `split` hands out the root of the lattice rather than a special case.
pub fn extends_path(prefix: &str, target: &str) -> bool {
    if !target.starts_with(prefix) {
        return false;
    }
    // Nothing left to separate: `/tmp/a` narrowed to itself, or a prefix
    // that already ends at a boundary.
    prefix.is_empty()
        || prefix.ends_with('/')
        || target.len() == prefix.len()
        || target.as_bytes()[prefix.len()] == b'/'
}

/// The plain labels owning a `World` discharges: the ones with no argument to
/// narrow. Named so that something that has to account for *every* label -- the
/// WASI import table (`wasi_imports.rs`) -- reads the same list the checker does
/// rather than a copy that can drift.
pub const WORLD_PLAIN_LABELS: &[&str] = &[
    "io_read",
    "io_write",
    "err_write",
    "heap",
    "args",
    "file_read",
    "file_write",
    "dir_read",
    "dir_write",
    "conn_accept",
    "conn_read",
    "conn_write",
    "udp_recv",
    "udp_send",
    "poll",
    "clock",
    "signals_read",
    "child_signal",
    "pipe_read",
    "pipe_write",
];

/// The labels the root `World` discharges the *unnarrowed* way (`FFI_ROOT`): each
/// is a label that names what it covers (a path prefix, a library, a host, a
/// signal set, a program).
pub const WORLD_ROOT_LABELS: &[&str] =
    &["ffi", "fs_read", "fs_write", "net_out", "net_in", "signals", "exec"];

/// What owning a value of this type authorises outright (§8.2).
///
/// Owning `Io` discharges `io_read`, `io_write` and `err_write`. Owning `World`
/// discharges everything a
/// `World` can be split into, because `split` is a function anyone holding
/// one may call — authority you can reach is authority you have.
///
/// A *borrowed* capability discharges nothing: `&!i Io` is precisely what
/// `[io_write]` on a signature means, so counting it here would make every row
/// empty and the whole section decoration.
pub(crate) fn discharged_by(defs: &[TypeDef], ty: &Type) -> Effects {
    let Type::Named(def, args) = ty else {
        return Effects::pure();
    };
    let index = def.0 as usize;
    if index >= defs.len() {
        return Effects::pure();
    }
    match index {
        // `docs/standard-input.md` §2.2: owning an `Io` outright discharges
        // every stream of the console, the way owning an `Fs` discharges
        // both of its directions. Three labels rather than two since
        // `docs/standard-error.md` §2.2.
        PRELUDE_IO => Effects::plain(["io_read", "io_write", "err_write"]),
        // `docs/heap.md` §2: one plain label, because a heap has no parts to
        // name and so nothing to narrow.
        PRELUDE_HEAP => Effects::plain(["heap"]),
        // `docs/file-handles.md` §4.1: one plain label, for the same reason
        // `heap` is plain. A file *does* have a name -- and the name was
        // spent at `open`, so re-attaching it here would mean a handle that
        // could not be passed to a function that had not been told where it
        // came from.
        PRELUDE_FILE => Effects::plain(["file_read", "file_write"]),
        // `docs/directory-handles.md` §2: the path was spent at `open_dir`,
        // so the handle's label names none, as `file_read` names none.
        PRELUDE_DIR => Effects::plain(["dir_read", "dir_write"]),
        // `docs/directory-listing.md` §3.3: a listing is reading beneath the
        // directory it came from.
        PRELUDE_DIR_LIST => Effects::plain(["dir_read"]),
        // `docs/native-sockets.md` §3: the same rule for the socket handles
        // -- the port was spent at `tcp_listen`, so the handle's own
        // labels carry no argument.
        PRELUDE_LISTENER => Effects::plain(["conn_accept"]),
        PRELUDE_CONN => Effects::plain(["conn_read", "conn_write"]),
        // `docs/udp.md` §3: the peer was spent at `udp_connect`, so the
        // handle's labels carry no argument, as a `Conn`'s do not.
        PRELUDE_UDP => Effects::plain(["udp_recv", "udp_send"]),
        // `docs/tty.md` §4: the row says the truth, argument-carrying and
        // narrowable exactly as `fs_read` is — a program reports
        // `tty_read("/dev")` for the prefix its capability grants.
        PRELUDE_TTY => match args.first() {
            Some(Type::Lit(prefix)) => Effects::new([
                Label { name: "tty_read".to_owned(), argument: Some(prefix.clone()) },
                Label { name: "tty_write".to_owned(), argument: Some(prefix.clone()) },
            ]),
            _ => Effects::plain(["tty_read", "tty_write"]),
        },
        // `docs/native-sockets.md` §4: observing handles already held, so
        // one plain label with nothing to narrow.
        PRELUDE_POLLER => Effects::plain(["poll"]),
        // `docs/native-sockets.md` §5: reading the time is an effect, and
        // owning the clock discharges it.
        PRELUDE_CLOCK => Effects::plain(["clock"]),
        // `docs/signals.md` section 2.1: a claim spent its set where it was
        // minted, so the handle's own label carries no argument, as a
        // `Conn`'s do not.
        PRELUDE_SIGNAL_WATCH => Effects::plain(["signals_read"]),
        // `docs/processes.md` §3.2: the program was named where the child was
        // started, so a child's and a channel's labels carry no argument.
        PRELUDE_CHILD => Effects::plain(["child_signal"]),
        PRELUDE_PIPE => Effects::plain(["pipe_read", "pipe_write"]),
        // Owning an `Exec(p)` discharges starting what lies under `p`, and
        // signalling a child it started: a `Child` comes from nowhere else.
        PRELUDE_EXEC => match args.first() {
            Some(Type::Lit(prefix)) => {
                let mut all = Effects::new([Label {
                    name: "exec".to_owned(),
                    argument: Some(prefix.clone()),
                }]);
                all.union(&Effects::plain(["child_signal"]));
                all
            }
            _ => Effects::pure(),
        },
        // Owning a `Signals("S")` discharges claiming `S` and reading what
        // it claimed; the root `Signals("")` covers every set (`Label::covers`
        // reads a set as a set).
        PRELUDE_SIGNALS => match args.first() {
            Some(Type::Lit(set)) => {
                let mut all = Effects::new([Label {
                    name: "signals".to_owned(),
                    argument: Some(set.clone()),
                }]);
                all.union(&Effects::plain(["signals_read"]));
                all
            }
            _ => Effects::pure(),
        },
        // `docs/arguments.md` §2: one plain label. There is one command
        // line and no part of it to name, so nothing to narrow.
        PRELUDE_ARGS => Effects::plain(["args"]),
        // A `World` is the root, so it discharges what every capability it
        // splits into discharges: the console, and the unnarrowed `Ffi`,
        // which covers every library there could be. Owning a `World` and
        // declaring `[]` is not a gap in the row — it is the parameter list
        // saying something stronger.
        PRELUDE_WORLD => {
            let mut all = Effects::plain(WORLD_PLAIN_LABELS.iter().copied());
            // `docs/net.md` §4.1, edition 2 only: `net_out` and `net_in`
            // are two more labels the root discharges the unnarrowed way
            // `ffi` and `fs_read`/`fs_write` already do. An edition-1
            // file's `World` discharges them just the same -- they are
            // simply labels no edition-1 body can ever perform, since it
            // has no way to name `Net` at all.
            for name in WORLD_ROOT_LABELS {
                all.union(&Effects::new([Label {
                    name: (*name).to_owned(),
                    argument: Some(FFI_ROOT.to_owned()),
                }]));
            }
            all
        }
        // Owning an `Ffi("libc")` discharges `ffi("libc")`, and owning the
        // root discharges `ffi("")`, which *covers* every library because
        // its holder can narrow to any of them.
        PRELUDE_FFI => match args.first() {
            Some(Type::Lit(library)) => {
                Effects::new([Label { name: "ffi".to_owned(), argument: Some(library.clone()) }])
            }
            _ => Effects::pure(),
        },
        // Owning an `Fs(prefix)` discharges reading *and* writing below it:
        // both are things its holder can reach without asking anyone
        // (`docs/filesystem.md` §1).
        PRELUDE_FS => match args.first() {
            Some(Type::Lit(prefix)) => {
                let mut all = Effects::new([
                    Label { name: "fs_read".to_owned(), argument: Some(prefix.clone()) },
                    Label { name: "fs_write".to_owned(), argument: Some(prefix.clone()) },
                ]);
                // `docs/file-handles.md` §4.1: and reading a handle it
                // opened. The only way to hold a `File` is to have held an
                // `Fs` -- `open_read` is where one comes from -- so the
                // capability that paid the prefix discharges the path-free
                // label that follows it. A function handed only a `File`
                // still declares `file_read`, which is the case the
                // authority report is for.
                all.union(&Effects::plain(["file_read", "file_write"]));
                // `docs/directory-handles.md` §2: the same for a directory
                // handle, which only `open_dir` on an `Fs` makes.
                all.union(&Effects::plain(["dir_read", "dir_write"]));
                all
            }
            _ => Effects::pure(),
        },
        // `docs/net.md` §2.1, §4.1: owning a `Net(bound)` discharges both
        // `net_out(bound)` and `net_in(bound)`, the same shape owning an
        // `Fs(prefix)` discharges both `fs_read(prefix)` and
        // `fs_write(prefix)` -- one bound, either operation.
        PRELUDE_NET => match args.first() {
            Some(Type::Lit(bound)) => {
                let mut all = Effects::new([
                    Label { name: "net_out".to_owned(), argument: Some(bound.clone()) },
                    Label { name: "net_in".to_owned(), argument: Some(bound.clone()) },
                ]);
                // The handles `tcp_listen` mints out of a `Net` carry
                // path-free labels, and the capability that paid the bound
                // discharges them -- `Fs` and `file_read`, again. `poll`
                // too: the only things a `Poller` can watch are the
                // sockets a `Net` made.
                all.union(&Effects::plain([
                    "conn_accept",
                    "conn_read",
                    "conn_write",
                    "udp_recv",
                    "udp_send",
                    "poll",
                ]));
                all
            }
            _ => Effects::pure(),
        },
        _ => Effects::pure(),
    }
}

/// The capability types the compiler provides (§8.1).
///
/// A capability is an *ordinary* `res` value — no special kind, no special
/// syntax, and no runtime representation beyond what its type says. These
/// two carry nothing, so they are zero-sized and threading one costs
/// literally nothing: the backend sees a value with no leaves.
///
/// `World` is the root of all authority and `Io` is the console. `Split` is
/// what `split` hands back when it consumes a `World`; it holds one field
/// today because `io` is the only effect anything performs. A capability for
/// an effect nothing can perform would be decoration, which is what §7.3
/// refuses for rows and what this refuses for the same reason — `Heap`, `Fs`
/// and `Ffi` arrive with allocation, the filesystem and FFI.
pub(crate) fn prelude_types(ast: &Ast, unifier: &mut Unifier) -> Vec<TypeDef> {
    let symbol = |name: &str| ast.symbols.get(name).expect("the prelude is interned by `Ast::new`");
    let span = Span::new(0, 0);
    let world = symbol("World");
    let io = symbol("Io");
    let ffi = symbol("Ffi");
    let fs = symbol("Fs");
    let heap = symbol("Heap");
    let arguments = symbol("Args");
    let boxed = symbol("Box");
    let split = symbol("Split");
    let file = symbol("File");
    let opened = symbol("Opened");
    let read_answer = symbol("Read");
    // `docs/net.md`: edition 2's outbound capability, and the `Split` that
    // carries it. Both reuse names already interned above (`Split`, `P`) --
    // `split_net` is a second, distinct declaration of the same source
    // name, not a rename.
    let net = symbol("Net");
    let split_net = symbol("Split");
    // `docs/threads.md` §2: `spawn`'s handle, edition 4 only.
    let thread = symbol("Thread");
    let thread_payload_param = symbol("T");
    let thread_ret_param = symbol("R");
    // `docs/native-sockets.md` §3: edition 5's handles and their answers.
    let listener = symbol("Listener");
    let conn = symbol("Conn");
    let listening = symbol("Listening");
    let accepted = symbol("Accepted");
    let received = symbol("Received");
    let sent = symbol("Sent");
    let dialed = symbol("Dialed");
    let poller = symbol("Poller");
    let polling = symbol("Polling");
    let clock = symbol("Clock");
    let clock_field = symbol("clock");
    let split_clock = symbol("Split");
    let attached = symbol("Attached");
    let done = symbol("Done");
    // `docs/signals.md`: edition 6's capability, its claim, the answer of
    // claiming, and the `Split` that carries the capability as its eighth
    // field.
    let signals = symbol("Signals");
    let signals_field = symbol("signals");
    let signal_watch = symbol("SignalWatch");
    let watching = symbol("Watching");
    let split_signals = symbol("Split");
    // `docs/directory-handles.md`: edition 6's directory handle and what
    // opening one answers.
    let dir = symbol("Dir");
    let dir_opened = symbol("DirOpened");
    // `docs/directory-listing.md`: a listing in progress, what starting one
    // answers, and what each step answers.
    let dir_list = symbol("DirList");
    let listing = symbol("Listing");
    let listed = symbol("Listed");
    let name_arm = symbol("Name");
    // What `dir_stat` answers.
    let dir_stat = symbol("DirStat");
    // `docs/processes.md` §3: edition 7's capability, the `Split` that
    // carries it as its ninth field, the child and the two ends of a channel,
    // what a child's stream is, and what the verbs answer.
    let exec = symbol("Exec");
    let exec_field = symbol("exec");
    let split_exec = symbol("Split");
    let child = symbol("Child");
    let pipe = symbol("Pipe");
    let child_end = symbol("ChildEnd");
    let stdio = symbol("Stdio");
    let piped = symbol("Piped");
    let spawned = symbol("Spawned");
    let exited = symbol("Exited");
    // `docs/udp.md` §3: edition 5's datagram socket and its answers.
    let udp = symbol("Udp");
    let udp_opened = symbol("UdpOpened");
    let datagram = symbol("Datagram");
    // `docs/tty.md` §3, edition 8: the serial-port capability, what opening
    // one answers, and the `Split` that carries it as its ninth field.
    // The handle is `Port` — one name cannot be two types, and the
    // capability's name is `Tty` (`Udp`'s precedent, not `Net`'s).
    let tty = symbol("Tty");
    let tty_field = symbol("tty");
    let split_tty = symbol("Split");
    let port = symbol("Port");
    let tty_opened = symbol("TtyOpened");
    let truncated_arm = symbol("Truncated");
    let null_arm = symbol("Null");
    let pipe_arm = symbol("Pipe");
    let file_arm = symbol("File");
    let code_arm = symbol("Code");
    let signaled_arm = symbol("Signaled");
    let again_arm = symbol("Again");
    let data_arm = symbol("Data");
    let wrote_arm = symbol("Wrote");
    let ok_arm = symbol("Ok");
    let failed_arm = symbol("Failed");
    let got_arm = symbol("Got");
    let end_arm = symbol("End");
    let library = symbol("L");
    let prefix = symbol("P");
    let boxed_param = symbol("B");
    // The *field* is `io`; the type it holds is `Io`. Two different names,
    // and interning them separately is what keeps them so.
    let io_field = symbol("io");
    let ffi_field = symbol("ffi");
    let fs_field = symbol("fs");
    let heap_field = symbol("heap");
    let args_field = symbol("args");
    let net_field = symbol("net");

    let world_def = unifier.declare("World");
    let io_def = unifier.declare("Io");
    let ffi_def = unifier.declare("Ffi");
    let fs_def = unifier.declare("Fs");
    let heap_def = unifier.declare("Heap");
    // The order of these calls is what fixes every `PRELUDE_*` constant, so
    // it matches the order of the definitions below, not the order the
    // names were added to the language.
    let box_def = unifier.declare("Box");
    let args_def = unifier.declare("Args");
    let split_def = unifier.declare("Split");
    let file_def = unifier.declare("File");
    let opened_def = unifier.declare("Opened");
    let read_def = unifier.declare("Read");
    // `PRELUDE_NET` and `PRELUDE_SPLIT_NET` (`ir.rs`): edition 2 only. A
    // second `declare("Split")` call is deliberate -- it is a distinct
    // `DefId` for the same source name, not a redeclaration, and `since`
    // below is what keeps an edition-1 file from ever resolving to it.
    let net_def = unifier.declare("Net");
    let split_net_def = unifier.declare("Split");
    // `PRELUDE_THREAD` (`ir.rs`): edition 4 only, appended last so no
    // earlier index moves.
    let thread_def = unifier.declare("Thread");
    // `PRELUDE_LISTENER` .. `PRELUDE_SENT` (`ir.rs`): edition 5 only, in
    // the order those constants fix.
    let listener_def = unifier.declare("Listener");
    let conn_def = unifier.declare("Conn");
    let listening_def = unifier.declare("Listening");
    let accepted_def = unifier.declare("Accepted");
    let received_def = unifier.declare("Received");
    let sent_def = unifier.declare("Sent");
    let dialed_def = unifier.declare("Dialed");
    let poller_def = unifier.declare("Poller");
    let polling_def = unifier.declare("Polling");
    // `PRELUDE_CLOCK` and `PRELUDE_SPLIT_CLOCK`: a third `Split`, for the
    // same reason edition 2 got a second (`editions.md` §7).
    let clock_def = unifier.declare("Clock");
    let split_clock_def = unifier.declare("Split");
    let attached_def = unifier.declare("Attached");
    let done_def = unifier.declare("Done");
    // `PRELUDE_SIGNALS` .. `PRELUDE_WATCHING`: edition 6, a fourth `Split`.
    let signals_def = unifier.declare("Signals");
    let split_signals_def = unifier.declare("Split");
    let signal_watch_def = unifier.declare("SignalWatch");
    let watching_def = unifier.declare("Watching");
    // `PRELUDE_DIR` and `PRELUDE_DIR_OPENED`: edition 6, appended last.
    let dir_def = unifier.declare("Dir");
    let dir_opened_def = unifier.declare("DirOpened");
    // `PRELUDE_DIR_LIST` .. `PRELUDE_LISTED`: edition 6, appended last.
    let dir_list_def = unifier.declare("DirList");
    let listing_def = unifier.declare("Listing");
    let listed_def = unifier.declare("Listed");
    // `PRELUDE_DIR_STAT`: edition 6, appended last.
    let dir_stat_def = unifier.declare("DirStat");
    // `PRELUDE_EXEC` .. `PRELUDE_EXITED`: edition 7, a fifth `Split`.
    let exec_def = unifier.declare("Exec");
    let split_exec_def = unifier.declare("Split");
    let child_def = unifier.declare("Child");
    let pipe_def = unifier.declare("Pipe");
    let child_end_def = unifier.declare("ChildEnd");
    let stdio_def = unifier.declare("Stdio");
    let piped_def = unifier.declare("Piped");
    let spawned_def = unifier.declare("Spawned");
    let exited_def = unifier.declare("Exited");
    // `PRELUDE_UDP` .. `PRELUDE_DATAGRAM`: edition 5, appended last.
    let udp_def = unifier.declare("Udp");
    let udp_opened_def = unifier.declare("UdpOpened");
    let datagram_def = unifier.declare("Datagram");
    let tty_def = unifier.declare("Tty");
    let split_tty_def = unifier.declare("Split");
    let port_def = unifier.declare("Port");
    let tty_opened_def = unifier.declare("TtyOpened");

    vec![
        TypeDef {
            name: world,
            def: world_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: Some(Mode::Res),
            kind: DefKind::Struct(Vec::new()),
            span,
            since: 1,
        },
        TypeDef {
            name: io,
            def: io_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: Some(Mode::Res),
            kind: DefKind::Struct(Vec::new()),
            span,
            since: 1,
        },
        // §8.4's capability, and the first one that carries data: `Ffi` is
        // indexed by the library it names, so `Ffi("libc")` and `Ffi("libm")`
        // are different types and a program cannot use one where the other
        // was granted.
        TypeDef {
            name: ffi,
            def: ffi_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: vec![library],
            bounds: Vec::new(),
            declared_mode: Some(Mode::Res),
            kind: DefKind::Struct(Vec::new()),
            span,
            since: 1,
        },
        // `docs/filesystem.md` §1: the same shape as `Ffi`, pointed at a
        // different kind of name. `Fs("/var")` narrows to
        // `Fs("/var/log/app")` by §7.4's prefix extension, and never back.
        TypeDef {
            name: fs,
            def: fs_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: vec![prefix],
            bounds: Vec::new(),
            declared_mode: Some(Mode::Res),
            kind: DefKind::Struct(Vec::new()),
            span,
            since: 1,
        },
        // `docs/heap.md` §2: the fifth capability, and the second that
        // carries nothing. A heap has no parts to name, so there is nothing
        // to narrow and no parameter to narrow it with.
        TypeDef {
            name: heap,
            def: heap_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: Some(Mode::Res),
            kind: DefKind::Struct(Vec::new()),
            span,
            since: 1,
        },
        // `docs/heap.md` §3: one value, one allocation. Declared `res`
        // whatever `B` is -- `B`'s mode says whether the *contents* must be
        // consumed, and the box is `res` because it owns an allocation,
        // which is true of a box of anything.
        //
        // It has no fields on purpose. What it owns is a pointer, and a
        // pattern that could name the pointer would be a way to end the
        // allocation without freeing it.
        TypeDef {
            name: boxed,
            def: box_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: vec![boxed_param],
            bounds: Vec::new(),
            declared_mode: Some(Mode::Res),
            kind: DefKind::Struct(Vec::new()),
            span,
            since: 1,
        },
        // `docs/arguments.md` §2: the sixth capability, and the third that
        // carries nothing. Reading argv is an effect because a function
        // whose behaviour depends on the command line should say so in its
        // type -- visibility, which is what a row is for, rather than
        // containment, which argv does not need.
        TypeDef {
            name: arguments,
            def: args_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: Some(Mode::Res),
            kind: DefKind::Struct(Vec::new()),
            span,
            since: 1,
        },
        // `res` by inference, because it holds five.
        TypeDef {
            name: split,
            def: split_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: None,
            kind: DefKind::Struct(vec![
                (io_field, Type::Named(io_def, Vec::new())),
                // Unnarrowed: the root authority to call out, which names no
                // library until someone narrows it to one.
                (ffi_field, Type::Named(ffi_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
                // Likewise unnarrowed: authority over no path until someone
                // narrows it to one.
                (fs_field, Type::Named(fs_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
                // Nothing to narrow: the heap is the heap.
                (heap_field, Type::Named(heap_def, Vec::new())),
                (args_field, Type::Named(args_def, Vec::new())),
            ]),
            span,
            since: 1,
        },
        // `docs/file-handles.md`: the handle. `res`, because a descriptor
        // is owned exactly once and `close` is what ends it -- the same
        // shape as every other resource here, and the reason §2's three
        // questions about linearity needed no new machinery.
        //
        // No fields, like `Box[T]`: what it owns is a descriptor, and a
        // pattern that could name it would be a program conjuring one.
        TypeDef {
            name: file,
            def: file_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: Some(Mode::Res),
            kind: DefKind::Struct(Vec::new()),
            span,
            since: 1,
        },
        // §2.1: what `open_read` answers, because `Result[T, E]` is
        // `std.result` and a builtin's signature is the prelude. Two arms,
        // and the type is a resource because one of them holds one.
        TypeDef {
            name: opened,
            def: opened_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: None,
            kind: DefKind::Enum(vec![
                (ok_arm, vec![Type::Named(file_def, Vec::new())]),
                (failed_arm, vec![Type::Int]),
            ]),
            span,
            since: 1,
        },
        // §3: three constructors because a read has three outcomes, and an
        // enum rather than a sentinel because a sentinel is how `getchar`
        // and `fs_read` came to disagree about what `-1` means. `Got(0)` is
        // not reachable -- a read that returns nothing is `End` or
        // `Failed`, never a zero.
        TypeDef {
            name: read_answer,
            def: read_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: None,
            kind: DefKind::Enum(vec![
                (got_arm, vec![Type::Int]),
                (end_arm, Vec::new()),
                (failed_arm, vec![Type::Int]),
            ]),
            span,
            since: 1,
        },
        // `docs/net.md` §1: the outbound capability, edition 2 only. Same
        // shape as `Fs` -- indexed by a prefix, narrowed and never widened
        // -- because `net_out`'s bound is `host:port` text matched the same
        // way `Fs`'s is a path.
        TypeDef {
            name: net,
            def: net_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: vec![prefix],
            bounds: Vec::new(),
            declared_mode: Some(Mode::Res),
            kind: DefKind::Struct(Vec::new()),
            span,
            since: 2,
        },
        // `docs/editions.md` §7: edition 2's `Split`, a distinct `DefId`
        // from the edition-1 one above so that naming it does not force
        // every edition-1 `main` to consume a field it never asked for.
        // `split()` picks between the two by the caller's edition
        // (`function.rs`); the two never coexist in one file's resolution.
        TypeDef {
            name: split_net,
            def: split_net_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: None,
            kind: DefKind::Struct(vec![
                (io_field, Type::Named(io_def, Vec::new())),
                (ffi_field, Type::Named(ffi_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
                (fs_field, Type::Named(fs_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
                (heap_field, Type::Named(heap_def, Vec::new())),
                (args_field, Type::Named(args_def, Vec::new())),
                (net_field, Type::Named(net_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
            ]),
            span,
            since: 2,
        },
        // `docs/threads.md` §2: `spawn`'s own handle. `res`, like `Box`
        // -- an obligation the type system tracks, discharged by
        // exactly one operation (`join`, the same "one consumer" shape
        // `unbox`/`close` already have for `Box`/`File`). No fields, for
        // the same reason `Box` has none: what it owns (a real OS
        // thread id) has nothing a pattern could usefully name, and
        // naming it would be a way to end the obligation without
        // joining.
        //
        // Two generic parameters, not one: `R` is `body`'s return type,
        // read back by `join`, but `T` -- the *payload* type -- is here
        // too, carried for no reason but its region. A payload that
        // borrows (`&r Io`, say) makes `Thread[T, R]`'s type mention
        // `r` through `Type::Named`'s existing, already-generic
        // `mentions`/`regions_into` walk into its own type arguments
        // (`crates/cancho-types/src/lib.rs`) -- the same escape check
        // a `borrow` block's own result is held to (`linearity-and-
        // effects.md` §5 rule 4), applied here with no new rule at all.
        // A handle whose payload borrowed `r` cannot survive past where
        // `r`'s block closes, which is `docs/threads.md` §3's whole
        // soundness argument, for free.
        TypeDef {
            name: thread,
            def: thread_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: vec![thread_payload_param, thread_ret_param],
            bounds: Vec::new(),
            declared_mode: Some(Mode::Res),
            kind: DefKind::Struct(Vec::new()),
            span,
            since: 4,
        },
        // `docs/native-sockets.md` §3: the two socket handles. `res`, one
        // descriptor leaf each, no fields -- `File`'s shape, for the reason
        // `File`'s is that shape: a descriptor is owned exactly once, and a
        // pattern that could name one would be conjuring someone else's.
        TypeDef {
            name: listener,
            def: listener_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: Some(Mode::Res),
            kind: DefKind::Struct(Vec::new()),
            span,
            since: 5,
        },
        TypeDef {
            name: conn,
            def: conn_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: Some(Mode::Res),
            kind: DefKind::Struct(Vec::new()),
            span,
            since: 5,
        },
        // What `tcp_listen` answers: `Opened`'s shape, for a listener.
        TypeDef {
            name: listening,
            def: listening_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: None,
            kind: DefKind::Enum(vec![
                (ok_arm, vec![Type::Named(listener_def, Vec::new())]),
                (failed_arm, vec![Type::Int]),
            ]),
            span,
            since: 5,
        },
        // `tcp_accept`: `Again` is its own constructor, not `Failed` of a
        // number whose value is 11 on Linux and 35 on macOS (§3).
        TypeDef {
            name: accepted,
            def: accepted_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: None,
            kind: DefKind::Enum(vec![
                (ok_arm, vec![Type::Named(conn_def, Vec::new())]),
                (again_arm, Vec::new()),
                (failed_arm, vec![Type::Int]),
            ]),
            span,
            since: 5,
        },
        // `conn_read`: `Read`'s three outcomes plus `Again`. `Data(0)` is
        // not reachable, as `Got(0)` is not.
        TypeDef {
            name: received,
            def: received_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: None,
            kind: DefKind::Enum(vec![
                (data_arm, vec![Type::Int]),
                (end_arm, Vec::new()),
                (again_arm, Vec::new()),
                (failed_arm, vec![Type::Int]),
            ]),
            span,
            since: 5,
        },
        // `conn_write`: how much the kernel took, or why it took none.
        TypeDef {
            name: sent,
            def: sent_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: None,
            kind: DefKind::Enum(vec![
                (wrote_arm, vec![Type::Int]),
                (again_arm, Vec::new()),
                (failed_arm, vec![Type::Int]),
            ]),
            span,
            since: 5,
        },
        // `tcp_connect`: `Listening`'s shape, for an outbound connection.
        // `Failed(-1)` is a name that did not resolve; any positive value
        // is the kernel's `errno`.
        TypeDef {
            name: dialed,
            def: dialed_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: None,
            kind: DefKind::Enum(vec![
                (ok_arm, vec![Type::Named(conn_def, Vec::new())]),
                (failed_arm, vec![Type::Int]),
            ]),
            span,
            since: 5,
        },
        // `docs/native-sockets.md` §4: the set of handles the kernel
        // watches. `res`, one descriptor leaf -- an `epoll`/`kqueue` fd --
        // and no literal form.
        TypeDef {
            name: poller,
            def: poller_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: Some(Mode::Res),
            kind: DefKind::Struct(Vec::new()),
            span,
            since: 5,
        },
        TypeDef {
            name: polling,
            def: polling_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: None,
            kind: DefKind::Enum(vec![
                (ok_arm, vec![Type::Named(poller_def, Vec::new())]),
                (failed_arm, vec![Type::Int]),
            ]),
            span,
            since: 5,
        },
        // `docs/native-sockets.md` §5: the clock, a capability like `Io` --
        // authority with nothing behind it, so zero-sized.
        TypeDef {
            name: clock,
            def: clock_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: Some(Mode::Res),
            kind: DefKind::Struct(Vec::new()),
            span,
            since: 5,
        },
        // Edition 5's `Split`: edition 2's six fields and `clock`.
        TypeDef {
            name: split_clock,
            def: split_clock_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: None,
            kind: DefKind::Struct(vec![
                (io_field, Type::Named(io_def, Vec::new())),
                (ffi_field, Type::Named(ffi_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
                (fs_field, Type::Named(fs_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
                (heap_field, Type::Named(heap_def, Vec::new())),
                (args_field, Type::Named(args_def, Vec::new())),
                (net_field, Type::Named(net_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
                (clock_field, Type::Named(clock_def, Vec::new())),
            ]),
            span,
            since: 5,
        },
        // `conn_attach`: `Dialed`'s shape. `Failed(EBADF)` is a ticket that
        // was never issued, was already redeemed, or was copied.
        TypeDef {
            name: attached,
            def: attached_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: None,
            kind: DefKind::Enum(vec![
                (ok_arm, vec![Type::Named(conn_def, Vec::new())]),
                (failed_arm, vec![Type::Int]),
            ]),
            span,
            since: 5,
        },
        // `docs/file-writes.md` section 4: what `file_write`, `file_sync`,
        // `file_truncate` and `file_size` answer. `Ok` carries the byte
        // count, the size, or `0`; `Failed` carries the `errno`, which a
        // sync needs to keep (section 5.1).
        TypeDef {
            name: done,
            def: done_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: None,
            kind: DefKind::Enum(vec![(ok_arm, vec![Type::Int]), (failed_arm, vec![Type::Int])]),
            span,
            since: 5,
        },
        // `docs/signals.md` section 2: the signal capability. Authority with
        // nothing behind it, so zero-sized like `Net`, and indexed by a
        // literal like `Net` is: the set it was narrowed to.
        TypeDef {
            name: signals,
            def: signals_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: vec![prefix],
            bounds: Vec::new(),
            declared_mode: Some(Mode::Res),
            kind: DefKind::Struct(Vec::new()),
            span,
            since: 6,
        },
        // Edition 6's `Split`: edition 5's seven fields and `signals`.
        TypeDef {
            name: split_signals,
            def: split_signals_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: None,
            kind: DefKind::Struct(vec![
                (io_field, Type::Named(io_def, Vec::new())),
                (ffi_field, Type::Named(ffi_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
                (fs_field, Type::Named(fs_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
                (heap_field, Type::Named(heap_def, Vec::new())),
                (args_field, Type::Named(args_def, Vec::new())),
                (net_field, Type::Named(net_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
                (clock_field, Type::Named(clock_def, Vec::new())),
                (signals_field, Type::Named(signals_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
            ]),
            span,
            since: 6,
        },
        // The claim: `res`, one descriptor leaf, no fields -- a `Conn`'s shape.
        TypeDef {
            name: signal_watch,
            def: signal_watch_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: Some(Mode::Res),
            kind: DefKind::Struct(Vec::new()),
            span,
            since: 6,
        },
        // `signals_watch`: `Polling`'s shape, for a claim.
        TypeDef {
            name: watching,
            def: watching_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: None,
            kind: DefKind::Enum(vec![
                (ok_arm, vec![Type::Named(signal_watch_def, Vec::new())]),
                (failed_arm, vec![Type::Int]),
            ]),
            span,
            since: 6,
        },
        // `docs/directory-handles.md` §2: a descriptor opened on a
        // directory. `File`'s shape: a resource with no fields a program
        // can name, closed only by `dir_close`.
        TypeDef {
            name: dir,
            def: dir_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: Some(Mode::Res),
            kind: DefKind::Struct(Vec::new()),
            span,
            since: 6,
        },
        // `open_dir` and `dir_enter`: `Opened`'s shape, for a directory.
        TypeDef {
            name: dir_opened,
            def: dir_opened_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: None,
            kind: DefKind::Enum(vec![
                (ok_arm, vec![Type::Named(dir_def, Vec::new())]),
                (failed_arm, vec![Type::Int]),
            ]),
            span,
            since: 6,
        },
        // `docs/directory-listing.md` §3.1: a stream of a directory's names.
        // `Dir`'s shape -- a resource with nothing a program can name,
        // closed only by `dir_list_close`.
        TypeDef {
            name: dir_list,
            def: dir_list_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: Some(Mode::Res),
            kind: DefKind::Struct(Vec::new()),
            span,
            since: 6,
        },
        // What `dir_list` answers: `DirOpened`'s shape, for a listing.
        TypeDef {
            name: listing,
            def: listing_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: None,
            kind: DefKind::Enum(vec![
                (ok_arm, vec![Type::Named(dir_list_def, Vec::new())]),
                (failed_arm, vec![Type::Int]),
            ]),
            span,
            since: 6,
        },
        // What `dir_next` answers: `Read`'s three outcomes, the first
        // carrying the name's length (its bytes are in the caller's buffer)
        // and its kind.
        TypeDef {
            name: listed,
            def: listed_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: None,
            kind: DefKind::Enum(vec![
                (name_arm, vec![Type::Int, Type::Int]),
                (end_arm, Vec::new()),
                (failed_arm, vec![Type::Int]),
            ]),
            span,
            since: 6,
        },
        // `docs/directory-listing.md` §3.2: what `dir_stat` answers -- the kind,
        // the size in bytes and the modification time in whole seconds, or the
        // `errno`.
        TypeDef {
            name: dir_stat,
            def: dir_stat_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: None,
            kind: DefKind::Enum(vec![
                (ok_arm, vec![Type::Int, Type::Int, Type::Int]),
                (failed_arm, vec![Type::Int]),
            ]),
            span,
            since: 6,
        },
        // `docs/processes.md` §3.1: the capability to start a program. Leaf-free
        // like `Fs`, and indexed by a path prefix the way `Fs` is.
        resource(exec, exec_def, vec![prefix], span, 7),
        // Edition 7's `Split`: edition 6's eight fields and `exec`.
        TypeDef {
            name: split_exec,
            def: split_exec_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: None,
            kind: DefKind::Struct(vec![
                (io_field, Type::Named(io_def, Vec::new())),
                (ffi_field, Type::Named(ffi_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
                (fs_field, Type::Named(fs_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
                (heap_field, Type::Named(heap_def, Vec::new())),
                (args_field, Type::Named(args_def, Vec::new())),
                (net_field, Type::Named(net_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
                (clock_field, Type::Named(clock_def, Vec::new())),
                (signals_field, Type::Named(signals_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
                (exec_field, Type::Named(exec_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
            ]),
            span,
            since: 7,
        },
        // A started process not yet reaped (one leaf, the pid); a channel's
        // end the parent holds and the one it hands the child (one
        // descriptor each). Resources with nothing a program can name.
        resource(child, child_def, Vec::new(), span, 7),
        resource(pipe, pipe_def, Vec::new(), span, 7),
        resource(child_end, child_end_def, Vec::new(), span, 7),
        // What one of the child's three streams is (§4.4).
        prelude_enum(
            stdio,
            stdio_def,
            vec![
                (null_arm, Vec::new()),
                (pipe_arm, vec![Type::Named(child_end_def, Vec::new())]),
                (file_arm, vec![Type::Named(file_def, Vec::new())]),
            ],
            span,
        ),
        // What `pipe_open` answers: both ends, or the `errno`.
        prelude_enum(
            piped,
            piped_def,
            vec![
                (
                    ok_arm,
                    vec![Type::Named(pipe_def, Vec::new()), Type::Named(child_end_def, Vec::new())],
                ),
                (failed_arm, vec![Type::Int]),
            ],
            span,
        ),
        // What `exec_spawn` answers.
        prelude_enum(
            spawned,
            spawned_def,
            vec![(ok_arm, vec![Type::Named(child_def, Vec::new())]), (failed_arm, vec![Type::Int])],
            span,
        ),
        // What `child_wait` answers (§4.7).
        prelude_enum(
            exited,
            exited_def,
            vec![
                (code_arm, vec![Type::Int]),
                (signaled_arm, vec![Type::Int]),
                (failed_arm, vec![Type::Int]),
            ],
            span,
        ),
        // `docs/udp.md` §3: a datagram socket. `res`, one descriptor leaf,
        // no fields -- a `Conn`'s shape, for a `Conn`'s reason.
        resource(udp, udp_def, Vec::new(), span, 5),
        // What `udp_connect` answers: `Dialed`'s shape. `Failed(-1)` is a name
        // that did not resolve; any positive value is the kernel's `errno`.
        edition_five_enum(
            udp_opened,
            udp_opened_def,
            vec![(ok_arm, vec![Type::Named(udp_def, Vec::new())]), (failed_arm, vec![Type::Int])],
            span,
        ),
        // What `udp_recv` answers: `Got(n)` where `n` may be 0 (an empty datagram
        // is a datagram), `Truncated(n)` where the datagram was `n` bytes and the
        // buffer held fewer, `Again` on a non-blocking socket with nothing waiting.
        edition_five_enum(
            datagram,
            datagram_def,
            vec![
                (got_arm, vec![Type::Int]),
                (truncated_arm, vec![Type::Int]),
                (again_arm, Vec::new()),
                (failed_arm, vec![Type::Int]),
            ],
            span,
        ),
        // `docs/tty.md` §3, edition 8: the serial-port capability, indexed
        // by a device-path prefix as `Fs` is — `Tty("/dev")` narrows to
        // `Tty("/dev/serial")` by the same §7.4 prefix extension and never
        // back, and an empty bound is refused as narrowing to the root.
        TypeDef {
            name: tty,
            def: tty_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: vec![prefix],
            bounds: Vec::new(),
            declared_mode: Some(Mode::Res),
            kind: DefKind::Struct(Vec::new()),
            span,
            since: 8,
        },
        // Edition 8's `Split`: edition 7's nine fields and `tty`, the
        // ninth field (`docs/tty.md` §6).
        TypeDef {
            name: split_tty,
            def: split_tty_def,
            module: PRELUDE_MODULE,
            public: true,
            generics: Vec::new(),
            bounds: Vec::new(),
            declared_mode: None,
            kind: DefKind::Struct(vec![
                (io_field, Type::Named(io_def, Vec::new())),
                (ffi_field, Type::Named(ffi_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
                (fs_field, Type::Named(fs_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
                (heap_field, Type::Named(heap_def, Vec::new())),
                (args_field, Type::Named(args_def, Vec::new())),
                (net_field, Type::Named(net_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
                (clock_field, Type::Named(clock_def, Vec::new())),
                (signals_field, Type::Named(signals_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
                (exec_field, Type::Named(exec_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
                (tty_field, Type::Named(tty_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
            ]),
            span,
            since: 8,
        },
        // The port itself (`docs/tty.md` §3's handle, named `Port` the way
        // `Udp` names a datagram socket rather than reusing the capability's
        // name — one name cannot be two types): `res`, one descriptor leaf,
        // no fields a program can name, closed only by `tty_close`.
        resource(port, port_def, Vec::new(), span, 8),
        // What `tty_open` answers: `UdpOpened`'s shape. `Failed(errno)` is
        // the program's question, not the compiler's.
        prelude_enum(
            tty_opened,
            tty_opened_def,
            vec![(ok_arm, vec![Type::Named(port_def, Vec::new())]), (failed_arm, vec![Type::Int])],
            span,
        ),
    ]
}

/// An edition-5 prelude enum (`docs/udp.md` §3): [`prelude_enum`] at the edition
/// the socket handles arrived in.
fn edition_five_enum(
    name: Symbol,
    def: DefId,
    arms: Vec<(Symbol, Vec<Type>)>,
    span: Span,
) -> TypeDef {
    TypeDef { since: 5, ..prelude_enum(name, def, arms, span) }
}

/// A prelude resource with no fields a program can name, at an edition: a
/// capability or a handle (`docs/processes.md` §3.1).
fn resource(name: Symbol, def: DefId, generics: Vec<Symbol>, span: Span, since: u32) -> TypeDef {
    TypeDef {
        name,
        def,
        module: PRELUDE_MODULE,
        public: true,
        generics,
        bounds: Vec::new(),
        declared_mode: Some(Mode::Res),
        kind: DefKind::Struct(Vec::new()),
        span,
        since,
    }
}

/// An edition-7 prelude enum (`docs/processes.md` §3.1).
fn prelude_enum(name: Symbol, def: DefId, arms: Vec<(Symbol, Vec<Type>)>, span: Span) -> TypeDef {
    TypeDef {
        name,
        def,
        module: PRELUDE_MODULE,
        public: true,
        generics: Vec::new(),
        bounds: Vec::new(),
        declared_mode: None,
        kind: DefKind::Enum(arms),
        span,
        since: 7,
    }
}

/// The first private type this one mentions, if any (`docs/modules.md`
/// §5).
///
/// A walk rather than a look at the head, because `Box[Secret]` and
/// `&r Secret` hide one just as surely as `Secret` does.
pub(crate) fn private_type_in(defs: &[TypeDef], module: u32, ty: &Type) -> Option<Symbol> {
    match ty {
        Type::Named(def, args) => {
            let declared = defs.iter().find(|d| d.def == *def)?;
            if declared.module == module && !declared.public {
                return Some(declared.name);
            }
            args.iter().find_map(|a| private_type_in(defs, module, a))
        }
        Type::Tuple(parts) => parts.iter().find_map(|p| private_type_in(defs, module, p)),
        Type::Ref { inner, .. } | Type::Slice(inner) => private_type_in(defs, module, inner),
        _ => None,
    }
}

/// Every `import` names a module the program declares, and no two bind
/// the same qualifier (`docs/modules.md` §4).
///
/// Checked here rather than where a qualified name is used, because an
/// import that names nothing is wrong whether or not anything reached
/// through it -- and an unused wrong import is exactly the one a
/// programmer wants told about.
pub(crate) fn check_imports(ast: &Ast) -> Result<(), Diagnostic> {
    for module in &ast.modules {
        let mut bound: Vec<Symbol> = Vec::new();
        for import in &module.imports {
            let path: Vec<&str> = import.path.iter().map(|s| ast.name_of(*s)).collect();
            if !ast.modules.iter().any(|m| m.path == import.path) {
                return Err(Diagnostic::new(
                    Rule::UnknownName,
                    format!(
                        "no module `{}` in this program; a module exists where a file declares it",
                        path.join(".")
                    ),
                    import.span,
                ));
            }
            if bound.contains(&import.alias) {
                return Err(Diagnostic::new(
                    Rule::DuplicateDeclaration,
                    format!(
                        "`{}` is already bound to another import here; name one of them with `as`",
                        ast.name_of(import.alias)
                    ),
                    import.span,
                ));
            }
            bound.push(import.alias);
        }
    }
    Ok(())
}

/// Collect every type declaration, in three passes.
///
/// Names first, so a member may mention a type declared later in the file;
/// then member types, which need those names; then the size check, which needs
/// every member type. Each pass needs the previous one complete, which is why
/// they are passes and not one loop.
pub(crate) fn collect_types(ast: &Ast, unifier: &mut Unifier) -> Result<Vec<TypeDef>, Diagnostic> {
    let mut defs: Vec<TypeDef> = prelude_types(ast, unifier);
    let predeclared = defs.len();

    for (index, item) in ast.items.iter().enumerate() {
        let (name_sym, noun, generics, bounds, declared_mode, public) = match item {
            Item::Struct(decl) => (
                decl.name,
                "struct",
                decl.generics.clone(),
                decl.bounds.clone(),
                decl.mode,
                decl.public,
            ),
            Item::Enum(decl) => (
                decl.name,
                "enum",
                decl.generics.clone(),
                decl.bounds.clone(),
                decl.mode,
                decl.public,
            ),
            Item::Fn(_) | Item::Extern(_) | Item::Static(_) => continue,
        };
        let item_id = ast::ItemId(index as u32);
        let module = ast.module_of(item_id);
        let span = ast.item_span(item_id);
        let name = ast.name_of(name_sym);

        // A prelude type is a built-in only for the files that can see it:
        // `Conn` is edition 5's, and an edition-1 file that declares its own
        // `Conn` is not redeclaring anything it can name.
        let edition = ast.edition_of(item_id);
        // `f32` is a type from edition 6 (`docs/f32.md` §6), so only a file
        // at that edition is redeclaring anything by declaring one.
        if matches!(name, "int" | "bool")
            || (name == "f32" && edition >= 6)
            || defs[..predeclared].iter().any(|d| d.name == name_sym && d.since <= edition)
        {
            return Err(Diagnostic::new(
                Rule::BuiltinRedeclared,
                format!("`{name}` is a built-in type and cannot be redeclared"),
                span,
            ));
        }
        // Scoped to the module: two modules may each declare a `Buffer`,
        // and that is the point of having them (`docs/modules.md` §3).
        // Within one module it is still an error, and the root is a module
        // like any other, so every program written before this is
        // unaffected.
        if defs.iter().any(|d| d.name == name_sym && d.module == module) {
            return Err(Diagnostic::new(
                Rule::DuplicateDeclaration,
                format!("type `{name}` is declared twice"),
                span,
            ));
        }

        // Ids are handed out in declaration order, so `DefId(i)` indexes
        // `defs[i]` and the backend can use the same numbering.
        let def = unifier.declare(name);
        let kind = match noun {
            "struct" => DefKind::Struct(Vec::new()),
            _ => DefKind::Enum(Vec::new()),
        };
        check_generic_names(ast, &generics, span)?;
        defs.push(TypeDef {
            name: name_sym,
            def,
            module,
            public,
            generics,
            bounds,
            declared_mode,
            kind,
            span,
            // A user declaration is never edition-gated -- only the
            // prelude's own additions are (`docs/editions.md` §7).
            since: 1,
        });
    }

    for (index, item) in ast.items.iter().enumerate() {
        let item_id = ast::ItemId(index as u32);
        let module = ast.module_of(item_id);
        let edition = ast.edition_of(item_id);
        match item {
            Item::Struct(decl) => {
                let position = defs
                    .iter()
                    .position(|d| d.name == decl.name && d.module == module)
                    .expect("declared");
                let span = defs[position].span;
                let mut fields: Vec<(Symbol, Type)> = Vec::new();
                for field in &decl.fields {
                    if fields.iter().any(|(n, _)| *n == field.name) {
                        return Err(Diagnostic::new(
                            Rule::DuplicateDeclaration,
                            format!(
                                "field `{}` is declared twice in `{}`",
                                ast.name_of(field.name),
                                ast.name_of(decl.name)
                            ),
                            span,
                        ));
                    }
                    let generics = defs[position].generics.clone();
                    let bounds = defs[position].bounds.clone();
                    fields.push((
                        field.name,
                        resolve_type(
                            Resolving { ast, defs: &defs, unifier, module, edition },
                            Params { names: &generics, bounds: &bounds },
                            &[],
                            field.ty,
                        )?,
                    ));
                }
                defs[position].kind = DefKind::Struct(fields);
            }
            Item::Enum(decl) => {
                let position = defs
                    .iter()
                    .position(|d| d.name == decl.name && d.module == module)
                    .expect("declared");
                let span = defs[position].span;
                if decl.variants.is_empty() {
                    return Err(Diagnostic::new(
                        Rule::EnumHasNoVariants,
                        format!(
                            "enum `{}` has no variants, so no value of it can ever exist",
                            ast.name_of(decl.name)
                        ),
                        span,
                    ));
                }
                let mut variants: Vec<(Symbol, Vec<Type>)> = Vec::new();
                for variant in &decl.variants {
                    if variants.iter().any(|(n, _)| *n == variant.name) {
                        return Err(Diagnostic::new(
                            Rule::DuplicateDeclaration,
                            format!(
                                "variant `{}` is declared twice in `{}`",
                                ast.name_of(variant.name),
                                ast.name_of(decl.name)
                            ),
                            span,
                        ));
                    }
                    let generics = defs[position].generics.clone();
                    let bounds = defs[position].bounds.clone();
                    let payload = variant
                        .payload
                        .iter()
                        .map(|ty| {
                            resolve_type(
                                Resolving { ast, defs: &defs, unifier, module, edition },
                                Params { names: &generics, bounds: &bounds },
                                &[],
                                *ty,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    variants.push((variant.name, payload));
                }
                defs[position].kind = DefKind::Enum(variants);
            }
            Item::Fn(_) | Item::Extern(_) | Item::Static(_) => {}
        }
    }

    for index in 0..defs.len() {
        let mut seen = vec![false; defs.len()];
        if reaches(&defs, index, index, &mut seen) {
            return Err(Diagnostic::new(
                Rule::InfiniteType,
                format!(
                    "type `{}` contains itself, so it has no finite size; put a `Box` on the path back to it",
                    ast.name_of(defs[index].name)
                ),
                defs[index].span,
            ));
        }
    }

    // §3: a declared `val` is a promise about the whole type, so a `res`
    // member breaks it. Inferring `res` instead would make the annotation
    // decorative; the declaration is refused so the promise means something.
    //
    // This runs after the acyclicity check because `mode_of` walks members
    // and relies on there being no cycle to walk forever in.
    for def in defs.iter() {
        if def.declared_mode != Some(Mode::Val) {
            continue;
        }
        let members: Vec<Type> = def.members().cloned().collect();
        // `docs/mode-polymorphism.md` §3: declaring the aggregate `val` is
        // a bound on its own parameters, so `Param(i)` is `val` *here*.
        // What makes that honest rather than circular is the instantiation
        // check in `resolve_type_at`, which refuses `Wrap[Box[int]]` where
        // `Wrap` is declared `val` -- without it, this check passes and the
        // promise is never kept (§2).
        let all_val: Vec<Option<Mode>> = vec![Some(Mode::Val); def.generics.len()];
        if let Some(member) =
            members.iter().find(|m| mode_of(&defs, unifier, &all_val, m) == Mode::Res)
        {
            return Err(Diagnostic::new(
                Rule::ModeBoundViolated,
                format!(
                    "`{}` is declared `val`, but it holds `{}`, which is `res`",
                    ast.name_of(def.name),
                    unifier.display(member)
                ),
                def.span,
            ));
        }
    }

    Ok(defs)
}
