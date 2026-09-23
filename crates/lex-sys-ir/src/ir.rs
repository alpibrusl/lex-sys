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

/// How many types the prelude declares. Written once, because a builtin's
/// signature indexes this table and a stale slice is a panic rather than a
/// diagnostic.
pub const PRELUDE_COUNT: usize = 11;

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
    pub fn covers(&self, other: &Label) -> bool {
        self.name == other.name
            && match (&self.argument, &other.argument) {
                (None, None) => true,
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
        args: Vec<Expr>,
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
    /// `malloc`, then the fill written into every element.
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
