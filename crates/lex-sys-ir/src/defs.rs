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
        PRELUDE_WORLD | PRELUDE_IO | PRELUDE_FFI | PRELUDE_FS | PRELUDE_HEAP | PRELUDE_ARGS
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
            // §8.2 again, and more sharply: a program that could write
            // `File { }` would be conjuring a descriptor, which is worse
            // than conjuring authority because the number would be someone
            // else's open file.
            | PRELUDE_FILE
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
        PRELUDE_WORLD | PRELUDE_IO | PRELUDE_FFI | PRELUDE_FS | PRELUDE_HEAP | PRELUDE_ARGS
    )
}

/// Is this a type whose only consumer is `close` (`docs/file-handles.md`)?
///
/// The same rule as [`released_only`] and [`unboxed_only`], pointed at the
/// third thing that owns something the language cannot see: a descriptor.
/// Destructuring a `File` would drop it without calling `close`, which is
/// a leak the kernel keeps rather than one the allocator does.
pub(crate) fn closed_only(def: DefId) -> bool {
    def.0 as usize == PRELUDE_FILE
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
        PRELUDE_FILE => Effects::plain(["file_read"]),
        // `docs/arguments.md` §2: one plain label. There is one command
        // line and no part of it to name, so nothing to narrow.
        PRELUDE_ARGS => Effects::plain(["args"]),
        // A `World` is the root, so it discharges what every capability it
        // splits into discharges: the console, and the unnarrowed `Ffi`,
        // which covers every library there could be. Owning a `World` and
        // declaring `[]` is not a gap in the row — it is the parameter list
        // saying something stronger.
        PRELUDE_WORLD => {
            let mut all =
                Effects::plain(["io_read", "io_write", "err_write", "heap", "args", "file_read"]);
            for name in ["ffi", "fs_read", "fs_write"] {
                all.union(&Effects::new([Label {
                    name: name.to_owned(),
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
                all.union(&Effects::plain(["file_read"]));
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
        },
    ]
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

        if matches!(name, "int" | "bool") || defs[..predeclared].iter().any(|d| d.name == name_sym)
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
        });
    }

    for (index, item) in ast.items.iter().enumerate() {
        let module = ast.module_of(ast::ItemId(index as u32));
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
                            Resolving { ast, defs: &defs, unifier, module },
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
                                Resolving { ast, defs: &defs, unifier, module },
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
