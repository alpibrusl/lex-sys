//! Resolution, type checking, and lowering to the M1 intermediate
//! representation.
//!
//! One walk over each function body does all three. It is the only pass that
//! reads a body, and everything a *program* can be refused for is refused
//! here, so the backend receives IR that cannot fail.
//!
//! Why one walk rather than a checker followed by a lowering pass: both need
//! the same scope stack and the same resolution of every name, and running
//! that twice means two places to disagree about what a name means. The type
//! vocabulary lives in `lex-sys-types`; this crate drives it.
//!
//! `docs/linearity-and-effects.md` §13 names three things M1 has to get right
//! before M2 can exist, and they are load-bearing here:
//!
//! 1. **A signature is complete and is the unit of checking.** Every parameter
//!    and return type is written. A body is checked against other functions'
//!    signatures, never against their bodies, so nothing is inferred across a
//!    boundary.
//! 2. **Types compare cheaply and canonically** — that is `lex-sys-types`.
//! 3. **The branch join is a real operation.** [`terminates`] is that join for
//!    control flow today; M2 adds a live set to the same shape.

use std::fmt;

use lex_sys_syntax::ast::{
    self, Ast, Block, Expr as AstExpr, ExprId, Item, Stmt as AstStmt, StmtId, Symbol, TypeExpr,
    TypeId,
};
use lex_sys_syntax::span::{Diagnostic, Span};
use lex_sys_types::{DefId, Region, Type, Unifier, UnifyError};

mod linear;

pub use linear::Mode;
use linear::{Event, Trace, mode_of};

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
pub const PRELUDE_SPLIT: usize = 3;

/// The library an unnarrowed `Ffi` names: none of them yet.
///
/// §7.4's narrowing is prefix extension — `Fs("/var")` becomes
/// `Fs("/var/log/app")` — and the empty string is the prefix of everything,
/// so the capability `split` hands out can still become any library and no
/// narrowed one can become another.
pub const FFI_ROOT: &str = "";

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

/// Functions the compiler provides rather than the program defining them.
///
/// M0/M1 scaffolding: `putchar` is how a program produces output before there
/// is any FFI. M2 replaces it with a capability-gated foreign call — output is
/// an effect, and an effect must be granted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Builtin {
    /// `putchar[&i](io: &!i Io, c: int) -> [io] int` — libc's `putchar`,
    /// byte for byte, behind the capability that authorises it.
    ///
    /// The `Io` is not passed to libc and has no runtime representation. It
    /// is there so that a function which does not hold one cannot call this,
    /// which is the whole safety story stated as a type (§8.2).
    PutChar,
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
    /// `len(s: &r [T]) -> [] int` — how many elements a slice has.
    ///
    /// Checked at the call site rather than through a written signature,
    /// because the element type is whatever the argument's is and a fixed
    /// signature cannot say that without a type parameter the builtin
    /// table has no way to bind.
    Len,
}

impl Builtin {
    pub const ALL: &'static [Builtin] = &[
        Builtin::PutChar,
        Builtin::Split,
        Builtin::Release,
        Builtin::Narrow,
        Builtin::WrappingAdd,
        Builtin::WrappingSub,
        Builtin::WrappingMul,
        Builtin::Len,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Builtin::PutChar => "putchar",
            Builtin::Split => "split",
            Builtin::Release => "release",
            Builtin::Narrow => "narrow",
            Builtin::WrappingAdd => "wrapping_add",
            Builtin::WrappingSub => "wrapping_sub",
            Builtin::WrappingMul => "wrapping_mul",
            Builtin::Len => "len",
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
            Builtin::PutChar => 1,
            _ => 0,
        }
    }

    /// How many region parameters the builtin takes, so a call site can
    /// instantiate them the same way it does for a written function (§5.1).
    pub fn regions(self) -> usize {
        match self {
            Builtin::PutChar => 1,
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
            Builtin::Split => (vec![named(PRELUDE_WORLD)], named(PRELUDE_SPLIT)),
            Builtin::WrappingAdd | Builtin::WrappingSub | Builtin::WrappingMul => {
                (vec![Type::Int, Type::Int], Type::Int)
            }
            Builtin::Len => (Vec::new(), Type::Int),
            // Both are checked at the call site rather than here, because a
            // fixed signature cannot say what they need. `release` ends any
            // capability, and there is more than one kind; `narrow` has an
            // argument *and* a result that depend on the literal written at
            // the call.
            Builtin::Release | Builtin::Narrow => (Vec::new(), Type::Unit),
        }
    }

    /// What performing this builtin costs a caller's row.
    ///
    /// `putchar` writes to the console, so it performs `io`. This is the
    /// *grounding* of the whole system: every `io` in every row above it
    /// traces back here, because a label nothing performs can never appear
    /// in an exact row (§7.3).
    pub fn effects(self) -> Effects {
        match self {
            Builtin::PutChar => Effects::plain(["io"]),
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
    /// `s[i]` — one element of a slice, bounds-checked (`defined-behaviour`
    /// §8). `element` is what comes back, which is how the backend knows
    /// the stride.
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        element: Type,
    },
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
    /// An enum value: which variant, and its payload.
    Enum {
        def: DefId,
        args: Vec<Type>,
        variant: u32,
        payload: Vec<Expr>,
    },
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Call {
        callee: Callee,
        args: Vec<Expr>,
    },
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
    pub n_params: u32,
    /// One entry per slot, parameters first. The backend reads these to pick a
    /// machine type, so every slot's type is resolved before it gets here.
    pub slots: Vec<Type>,
    pub ret: Type,
    pub body: Vec<Stmt>,
}

impl Func {
    pub fn n_slots(&self) -> u32 {
        self.slots.len() as u32
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

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Program {
    pub funcs: Vec<Func>,
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

/// A function's signature: what a caller is checked against, and all a caller
/// is ever checked against.
struct Signature {
    /// The declared effect row (§7.2): written at the boundary, never
    /// inferred across one. A caller is checked against this and never
    /// against the body that justifies it.
    effects: Effects,
    /// Region parameters, in declaration order; `Region::Param(i)` is the
    /// `i`th. Not part of monomorphisation: see `lower_function`.
    regions: Vec<Symbol>,
    /// `where a <= b` as indices into `regions`, meaning `b` outlives `a`.
    outlives: Vec<(u32, u32)>,
    name: Symbol,
    /// Type parameters in declaration order; `Type::Param(i)` is the `i`th.
    generics: Vec<Symbol>,
    params: Vec<Type>,
    ret: Type,
    /// Position of the declaration among the unit's items.
    item: usize,
}

/// One monomorphic copy of a function: which function, and the type arguments
/// it was instantiated at. A non-generic function has exactly one, with no
/// arguments.
struct Instance {
    signature: usize,
    args: Vec<Type>,
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
struct Mono {
    instances: Vec<Instance>,
    pending: Vec<usize>,
    /// False while checking a generic function rigidly, where instantiations
    /// are hypothetical and must not be emitted.
    recording: bool,
}

impl Mono {
    fn new(recording: bool) -> Self {
        Self { instances: Vec::new(), pending: Vec::new(), recording }
    }

    /// The id of this instance, creating it if it is new.
    fn request(&mut self, signature: usize, args: Vec<Type>) -> FuncId {
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
fn instance_name(base: &str, args: &[Type], unifier: &Unifier) -> String {
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

pub(crate) struct TypeDef {
    name: Symbol,
    def: DefId,
    /// Type parameters in declaration order; `Type::Param(i)` is the `i`th.
    generics: Vec<Symbol>,
    /// The mode the declaration wrote, if it wrote one. `None` means the mode
    /// is whatever the members make it (§3).
    pub(crate) declared_mode: Option<Mode>,
    kind: DefKind,
    span: Span,
}

impl TypeDef {
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
/// M1 has no references, so a type that contains itself — directly or through
/// others — has no finite size. There is no representation to pick and no
/// depth to stop at, so it is refused rather than approximated. That covers
/// `enum List { Nil, Cons(int, List) }` as much as a self-referential struct:
/// the classic linked list needs an indirection the language does not have
/// yet.
fn reaches(defs: &[TypeDef], from: usize, target: usize, seen: &mut [bool]) -> bool {
    if seen[from] {
        return false;
    }
    seen[from] = true;
    defs[from].members().any(|ty| match ty {
        Type::Named(def, _) => {
            let next = def.0 as usize;
            next == target || reaches(defs, next, target, seen)
        }
        _ => false,
    })
}

/// Does a value of this type occupy no machine values at all?
///
/// §8.1: "capabilities erase at compile time except where they carry data".
/// A struct with no fields has no leaves, so threading one is free -- which
/// is a claim worth a test rather than a comment.
pub fn leaf_free(ty: &Type) -> bool {
    matches!(ty, Type::Named(def, _)
        if matches!(def.0 as usize, PRELUDE_WORLD | PRELUDE_IO | PRELUDE_FFI))
}

/// Is this one of the prelude's capability types?
///
/// §8.2: authority comes from exactly one place, and a program that could
/// *write* `Io { }` would have an ambient constructor by another name — the
/// very thing "no `Io::global()`, no `unsafe { }` that conjures one" rules
/// out. So a capability has no literal form, and the only `Io` in existence
/// is the one the runtime handed to `main` inside a `World`.
fn is_capability(def: DefId) -> bool {
    matches!(def.0 as usize, PRELUDE_WORLD | PRELUDE_IO | PRELUDE_FFI | PRELUDE_SPLIT)
}

/// Is this a capability whose only consumer is `release`?
///
/// `Split` is not: taking it apart is the whole point of having one.
/// `World` and `Io` are, because destructuring either would destroy
/// authority without naming the function that knows how (§4.1) — and for
/// `World` and `Io`, which carry no fields, it would do it silently.
fn released_only(def: DefId) -> bool {
    matches!(def.0 as usize, PRELUDE_WORLD | PRELUDE_IO | PRELUDE_FFI)
}

/// What owning a value of this type authorises outright (§8.2).
///
/// Owning `Io` discharges `io`. Owning `World` discharges everything a
/// `World` can be split into, because `split` is a function anyone holding
/// one may call — authority you can reach is authority you have.
///
/// A *borrowed* capability discharges nothing: `&!i Io` is precisely what
/// `[io]` on a signature means, so counting it here would make every row
/// empty and the whole section decoration.
fn discharged_by(defs: &[TypeDef], ty: &Type) -> Effects {
    let Type::Named(def, args) = ty else {
        return Effects::pure();
    };
    let index = def.0 as usize;
    if index >= defs.len() {
        return Effects::pure();
    }
    match index {
        PRELUDE_IO => Effects::plain(["io"]),
        // A `World` is the root, so it discharges what every capability it
        // splits into discharges: the console, and the unnarrowed `Ffi`,
        // which covers every library there could be. Owning a `World` and
        // declaring `[]` is not a gap in the row — it is the parameter list
        // saying something stronger.
        PRELUDE_WORLD => {
            let mut all = Effects::plain(["io"]);
            all.union(&Effects::new([Label {
                name: "ffi".to_owned(),
                argument: Some(FFI_ROOT.to_owned()),
            }]));
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
fn prelude_types(ast: &Ast, unifier: &mut Unifier) -> Vec<TypeDef> {
    let symbol = |name: &str| ast.symbols.get(name).expect("the prelude is interned by `Ast::new`");
    let span = Span::new(0, 0);
    let world = symbol("World");
    let io = symbol("Io");
    let ffi = symbol("Ffi");
    let split = symbol("Split");
    let library = symbol("L");
    // The *field* is `io`; the type it holds is `Io`. Two different names,
    // and interning them separately is what keeps them so.
    let io_field = symbol("io");
    let ffi_field = symbol("ffi");

    let world_def = unifier.declare("World");
    let io_def = unifier.declare("Io");
    let ffi_def = unifier.declare("Ffi");
    let split_def = unifier.declare("Split");

    vec![
        TypeDef {
            name: world,
            def: world_def,
            generics: Vec::new(),
            declared_mode: Some(Mode::Res),
            kind: DefKind::Struct(Vec::new()),
            span,
        },
        TypeDef {
            name: io,
            def: io_def,
            generics: Vec::new(),
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
            generics: vec![library],
            declared_mode: Some(Mode::Res),
            kind: DefKind::Struct(Vec::new()),
            span,
        },
        // `res` by inference, because it holds two.
        TypeDef {
            name: split,
            def: split_def,
            generics: Vec::new(),
            declared_mode: None,
            kind: DefKind::Struct(vec![
                (io_field, Type::Named(io_def, Vec::new())),
                // Unnarrowed: the root authority to call out, which names no
                // library until someone narrows it to one.
                (ffi_field, Type::Named(ffi_def, vec![Type::Lit(FFI_ROOT.to_owned())])),
            ]),
            span,
        },
    ]
}

/// Collect every type declaration, in three passes.
///
/// Names first, so a member may mention a type declared later in the file;
/// then member types, which need those names; then the size check, which needs
/// every member type. Each pass needs the previous one complete, which is why
/// they are passes and not one loop.
fn collect_types(ast: &Ast, unifier: &mut Unifier) -> Result<Vec<TypeDef>, Diagnostic> {
    let mut defs: Vec<TypeDef> = prelude_types(ast, unifier);
    let predeclared = defs.len();

    for (index, item) in ast.items.iter().enumerate() {
        let (name_sym, noun, generics, declared_mode) = match item {
            Item::Struct(decl) => (decl.name, "struct", decl.generics.clone(), decl.mode),
            Item::Enum(decl) => (decl.name, "enum", decl.generics.clone(), decl.mode),
            Item::Fn(_) | Item::Extern(_) => continue,
        };
        let span = ast.item_span(ast::ItemId(index as u32));
        let name = ast.name_of(name_sym);

        if matches!(name, "int" | "bool") || defs[..predeclared].iter().any(|d| d.name == name_sym)
        {
            return Err(Diagnostic::new(
                format!("`{name}` is a built-in type and cannot be redeclared"),
                span,
            ));
        }
        if let Some(previous) = defs.iter().find(|d| d.name == name_sym) {
            let _ = previous;
            return Err(Diagnostic::new(format!("type `{name}` is declared twice"), span));
        }

        // Ids are handed out in declaration order, so `DefId(i)` indexes
        // `defs[i]` and the backend can use the same numbering.
        let def = unifier.declare(name);
        let kind = match noun {
            "struct" => DefKind::Struct(Vec::new()),
            _ => DefKind::Enum(Vec::new()),
        };
        check_generic_names(ast, &generics, span)?;
        defs.push(TypeDef { name: name_sym, def, generics, declared_mode, kind, span });
    }

    for item in ast.items.iter() {
        match item {
            Item::Struct(decl) => {
                let position = defs.iter().position(|d| d.name == decl.name).expect("declared");
                let span = defs[position].span;
                let mut fields: Vec<(Symbol, Type)> = Vec::new();
                for field in &decl.fields {
                    if fields.iter().any(|(n, _)| *n == field.name) {
                        return Err(Diagnostic::new(
                            format!(
                                "field `{}` is declared twice in `{}`",
                                ast.name_of(field.name),
                                ast.name_of(decl.name)
                            ),
                            span,
                        ));
                    }
                    let generics = defs[position].generics.clone();
                    fields.push((field.name, resolve_type(ast, &defs, &generics, &[], field.ty)?));
                }
                defs[position].kind = DefKind::Struct(fields);
            }
            Item::Enum(decl) => {
                let position = defs.iter().position(|d| d.name == decl.name).expect("declared");
                let span = defs[position].span;
                if decl.variants.is_empty() {
                    return Err(Diagnostic::new(
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
                            format!(
                                "variant `{}` is declared twice in `{}`",
                                ast.name_of(variant.name),
                                ast.name_of(decl.name)
                            ),
                            span,
                        ));
                    }
                    let generics = defs[position].generics.clone();
                    let payload = variant
                        .payload
                        .iter()
                        .map(|ty| resolve_type(ast, &defs, &generics, &[], *ty))
                        .collect::<Result<Vec<_>, _>>()?;
                    variants.push((variant.name, payload));
                }
                defs[position].kind = DefKind::Enum(variants);
            }
            Item::Fn(_) | Item::Extern(_) => {}
        }
    }

    for index in 0..defs.len() {
        let mut seen = vec![false; defs.len()];
        if reaches(&defs, index, index, &mut seen) {
            return Err(Diagnostic::new(
                format!(
                    "type `{}` contains itself, so it has no finite size (M1 has no references)",
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
        if let Some(member) = members.iter().find(|m| mode_of(&defs, unifier, m) == Mode::Res) {
            return Err(Diagnostic::new(
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

/// Resolve and check an AST, producing IR a backend can lower without failing.
pub fn lower(ast: &Ast) -> Result<Program, Diagnostic> {
    let mut unifier = Unifier::new();
    let defs = collect_types(ast, &mut unifier)?;

    // Every function is visible to every other, so collect signatures before
    // checking any body. Definition order in the file is irrelevant, and no
    // body is ever consulted to type a call.
    // §8.4: a foreign signature is written once, here, and every caller is
    // checked against it exactly as against a written function's.
    let mut externs: Vec<ExternFn> = Vec::new();
    for (index, item) in ast.items.iter().enumerate() {
        let Item::Extern(decl) = item else { continue };
        let name = ast.name_of(decl.name);
        let span = ast.item_span(ast::ItemId(index as u32));
        if Builtin::from_name(name).is_some() {
            return Err(Diagnostic::new(
                format!("`{name}` is a builtin and cannot be declared foreign"),
                span,
            ));
        }
        if externs.iter().any(|e| e.name == name) {
            return Err(Diagnostic::new(
                format!("foreign function `{name}` is declared twice"),
                span,
            ));
        }
        let region_scope = check_region_names(ast, &decl.regions, &[], span)?;
        let params = decl
            .params
            .iter()
            .map(|p| resolve_type(ast, &defs, &[], &region_scope, p.ty))
            .collect::<Result<Vec<_>, _>>()?;
        let ret = resolve_type(ast, &defs, &[], &region_scope, decl.ret)?;
        let declared =
            Effects::new(decl.effects.iter().map(|e| Label {
                name: ast.name_of(e.name).to_owned(),
                argument: e.argument.clone(),
            }));

        // §8.4: what crosses the boundary is what C can name. Aggregates
        // have no layout contract here yet, and a reference to anything but
        // a capability would be a pointer this compiler has not promised to
        // lay out — so both are refused at the declaration, where the author
        // can still write something else, rather than at the call.
        for (param, decl_param) in params.iter().zip(&decl.params) {
            let what = ast.name_of(decl_param.name);
            match param {
                Type::Int | Type::Bool => {}
                Type::Ref { inner, .. } if matches!(inner.as_ref(), Type::Named(def, _) if is_capability(*def)) =>
                    {}
                Type::Ref { .. } => {
                    return Err(Diagnostic::new(
                        format!(
                            "`{name}` takes `{what}` by reference, but the only reference that crosses a foreign boundary is a borrowed capability: C is not told about regions"
                        ),
                        span,
                    ));
                }
                other => {
                    return Err(Diagnostic::new(
                        format!(
                            "`{name}` takes `{what}` of type `{}`, which has no agreed layout across a foreign boundary; a foreign parameter is `int`, `bool`, or a borrowed capability",
                            unifier.display(other)
                        ),
                        span,
                    ));
                }
            }
        }
        if !matches!(ret, Type::Int | Type::Bool | Type::Unit) {
            return Err(Diagnostic::new(
                format!(
                    "`{name}` returns `{}`, which has no agreed layout across a foreign boundary; a foreign result is `int`, `bool`, or `()`",
                    unifier.display(&ret)
                ),
                span,
            ));
        }

        // §8.4: the capability is the only way to reach the foreign call, so
        // the row and the capability parameters have to agree. A declaration
        // that named one library and borrowed another would be the one place
        // a foreign signature is written, written wrong.
        let mut authorised = Effects::pure();
        for param in &params {
            if let Type::Ref { inner, .. } = param {
                authorised.union(&discharged_by(&defs, inner));
            }
        }
        for param in &params {
            let Type::Ref { inner, .. } = param else { continue };
            if let Type::Named(def, args) = inner.as_ref()
                && def.0 as usize == PRELUDE_FFI
                && matches!(args.first(), Some(Type::Lit(library)) if library == FFI_ROOT)
            {
                return Err(Diagnostic::new(
                    format!(
                        "`{name}` borrows the unnarrowed `Ffi(\"\")`, which names no library; a foreign declaration names the library it calls into, so narrow before declaring"
                    ),
                    span,
                ));
            }
        }
        if let Some(label) = declared.missing_from(&authorised) {
            return Err(Diagnostic::new(
                format!(
                    "`{name}` declares `{label}` but holds no capability that authorises it; a foreign call is reached through the capability that names its library"
                ),
                span,
            ));
        }
        if let Some(label) = authorised.missing_from(&declared) {
            return Err(Diagnostic::new(
                format!(
                    "`{name}` borrows a capability authorising `{label}`, which its row {declared} does not declare"
                ),
                span,
            ));
        }

        externs.push(ExternFn {
            name: name.to_owned(),
            symbol: decl.symbol.clone(),
            params,
            effects: declared,
            ret,
        });
    }

    let mut signatures: Vec<Signature> = Vec::new();
    for (index, item) in ast.items.iter().enumerate() {
        let Item::Fn(decl) = item else { continue };
        let name = ast.name_of(decl.name);
        let span = ast.item_span(ast::ItemId(index as u32));

        if Builtin::from_name(name).is_some() {
            return Err(Diagnostic::new(
                format!("`{name}` is a builtin and cannot be redefined"),
                span,
            ));
        }
        if signatures.iter().any(|s| s.name == decl.name) {
            return Err(Diagnostic::new(format!("function `{name}` is defined twice"), span));
        }
        // A foreign declaration and a written function are two answers to
        // the same call, and a call resolves to one thing.
        if externs.iter().any(|e| e.name == name) {
            return Err(Diagnostic::new(
                format!("`{name}` is already declared foreign, so this name is taken"),
                span,
            ));
        }
        check_generic_names(ast, &decl.generics, span)?;
        let region_scope = check_region_names(ast, &decl.regions, &decl.generics, span)?;

        // `where a <= b` names two region parameters, resolved to their
        // positions so the relation is integers from here on (§5.2).
        let mut outlives = Vec::new();
        for (inner, outer) in &decl.outlives {
            let position = |sym: &Symbol| decl.regions.iter().position(|r| r == sym);
            let (Some(a), Some(b)) = (position(inner), position(outer)) else {
                let missing = if position(inner).is_none() {
                    ast.name_of(*inner)
                } else {
                    ast.name_of(*outer)
                };
                return Err(Diagnostic::new(
                    format!(
                        "`{missing}` is not a region parameter of `{name}`; a `where` clause relates the regions the declaration takes"
                    ),
                    span,
                ));
            };
            outlives.push((a as u32, b as u32));
        }

        let mut seen: Vec<Symbol> = Vec::new();
        let mut params = Vec::new();
        for param in &decl.params {
            if seen.contains(&param.name) {
                return Err(Diagnostic::new(
                    format!("parameter `{}` is bound twice", ast.name_of(param.name)),
                    span,
                ));
            }
            seen.push(param.name);
            params.push(resolve_type(ast, &defs, &decl.generics, &region_scope, param.ty)?);
        }

        let ret = resolve_type(ast, &defs, &decl.generics, &region_scope, decl.ret)?;
        signatures.push(Signature {
            name: decl.name,
            generics: decl.generics.clone(),
            regions: decl.regions.clone(),
            outlives,
            effects: Effects::new(decl.effects.iter().map(|e| Label {
                name: ast.name_of(e.name).to_owned(),
                argument: e.argument.clone(),
            })),
            params,
            ret,
            item: index,
        });
    }

    // Pass 1: check each generic function once, with its parameters rigid.
    //
    // Without this an unused generic function is never checked at all, since
    // pass 2 only reaches what is called. Rigid parameters are also the
    // stronger check: a body that type-checks for every `T` is checked once,
    // rather than once per instantiation and never for the `T` nobody used.
    for (index, signature) in signatures.iter().enumerate() {
        if signature.generics.is_empty() {
            continue;
        }
        let rigid: Vec<Type> = (0..signature.generics.len() as u32).map(Type::Param).collect();
        let mut checking = Mono::new(false);
        lower_function(
            ast,
            &defs,
            &signatures,
            &externs,
            &mut unifier,
            index,
            &rigid,
            &mut checking,
        )?;
    }

    // Pass 2: emit a copy of every function actually reachable, starting from
    // the ones that need no type arguments.
    let mut mono = Mono::new(true);
    for (index, signature) in signatures.iter().enumerate() {
        if signature.generics.is_empty() {
            mono.request(index, Vec::new());
        }
    }

    let mut funcs: Vec<Option<Func>> = Vec::new();
    while let Some(instance) = mono.pending.pop() {
        let (signature, args) =
            (mono.instances[instance].signature, mono.instances[instance].args.clone());
        let func = lower_function(
            ast,
            &defs,
            &signatures,
            &externs,
            &mut unifier,
            signature,
            &args,
            &mut mono,
        )?;
        if funcs.len() <= instance {
            funcs.resize_with(instance + 1, || None);
        }
        funcs[instance] = Some(func);
    }

    Ok(Program {
        funcs: funcs.into_iter().map(|f| f.expect("every requested instance is lowered")).collect(),
        externs,
        types: defs
            .iter()
            .map(|d| match &d.kind {
                DefKind::Struct(fields) => TypeInfo::Struct {
                    name: ast.name_of(d.name).to_owned(),
                    fields: fields
                        .iter()
                        .map(|(n, t)| (ast.name_of(*n).to_owned(), t.clone()))
                        .collect(),
                },
                DefKind::Enum(variants) => TypeInfo::Enum {
                    name: ast.name_of(d.name).to_owned(),
                    variants: variants
                        .iter()
                        .map(|(n, p)| (ast.name_of(*n).to_owned(), p.clone()))
                        .collect(),
                },
            })
            .collect(),
    })
}

/// Replace every inference variable in a lowered body with what it was solved
/// to.
///
/// Types are written into the IR while checking is still in progress, so a
/// node can capture a variable that a later statement settles — `let x:
/// Option[int] = Option::None;` builds the value before the annotation has
/// said what `T` is. One walk afterwards settles them all.
fn settle_types(stmts: &mut [Stmt], unifier: &Unifier) {
    for stmt in stmts {
        match stmt {
            Stmt::Store { place, value } => {
                match place {
                    Place::Field { base, args, .. } => {
                        settle_expr(base, unifier);
                        for arg in args.iter_mut() {
                            *arg = unifier.resolve(arg);
                        }
                    }
                    Place::Element { base, index, element } => {
                        settle_expr(base, unifier);
                        settle_expr(index, unifier);
                        *element = unifier.resolve(element);
                    }
                    Place::Slot(_) => {}
                }
                settle_expr(value, unifier);
            }
            Stmt::Eval(value) | Stmt::Return(value) => settle_expr(value, unifier),
            Stmt::If { cond, then_body, else_body } => {
                settle_expr(cond, unifier);
                settle_types(then_body, unifier);
                settle_types(else_body, unifier);
            }
            Stmt::While { cond, body } => {
                settle_expr(cond, unifier);
                settle_types(body, unifier);
            }
            Stmt::Borrow { body, .. } | Stmt::Region { body, .. } => settle_types(body, unifier),
            Stmt::Match { scrutinee, args, arms, .. } => {
                settle_expr(scrutinee, unifier);
                for arg in args.iter_mut() {
                    *arg = unifier.resolve(arg);
                }
                for arm in arms {
                    settle_types(&mut arm.body, unifier);
                }
            }
        }
    }
}

fn settle_expr(expr: &mut Expr, unifier: &Unifier) {
    match expr {
        Expr::Int(_) | Expr::Bool(_) | Expr::Load(_) => {}
        Expr::Neg(inner) | Expr::Not(inner) => settle_expr(inner, unifier),
        Expr::Bin { lhs, rhs, .. } => {
            settle_expr(lhs, unifier);
            settle_expr(rhs, unifier);
        }
        Expr::Call { args, .. } => {
            for arg in args {
                settle_expr(arg, unifier);
            }
        }
        Expr::Struct { fields, .. } => {
            for field in fields {
                settle_expr(field, unifier);
            }
        }
        Expr::Alloc { ty, value, .. } => {
            *ty = unifier.resolve(ty);
            settle_expr(value, unifier);
        }
        Expr::AllocSlice { element, count, fill, .. } => {
            *element = unifier.resolve(element);
            settle_expr(count, unifier);
            settle_expr(fill, unifier);
        }
        Expr::Index { base, index, element } => {
            *element = unifier.resolve(element);
            settle_expr(base, unifier);
            settle_expr(index, unifier);
        }
        Expr::Len(inner) => settle_expr(inner, unifier),
        Expr::FieldRef { base, args, .. } => {
            settle_expr(base, unifier);
            for arg in args.iter_mut() {
                *arg = unifier.resolve(arg);
            }
        }
        Expr::Field { base, args, .. } => {
            settle_expr(base, unifier);
            for arg in args.iter_mut() {
                *arg = unifier.resolve(arg);
            }
        }
        Expr::Enum { args, payload, .. } => {
            for arg in args.iter_mut() {
                *arg = unifier.resolve(arg);
            }
            for value in payload {
                settle_expr(value, unifier);
            }
        }
    }
}

/// Check and lower one function at one instantiation.
#[allow(clippy::too_many_arguments)]
fn lower_function(
    ast: &Ast,
    defs: &[TypeDef],
    signatures: &[Signature],
    externs: &[ExternFn],
    unifier: &mut Unifier,
    index: usize,
    args: &[Type],
    mono: &mut Mono,
) -> Result<Func, Diagnostic> {
    let signature = &signatures[index];
    let Item::Fn(decl) = &ast.items[signature.item] else {
        unreachable!("a signature always names a function");
    };

    // Regions are *not* substituted here and a function is not copied per
    // region: a reference is a pointer and has no idea which block it came
    // from, so there is nothing to specialise. Region parameters stay rigid
    // inside the body and are instantiated at each call site instead (§5.1).
    let params: Vec<Type> = signature.params.iter().map(|t| t.substitute(args, &[])).collect();
    let ret = signature.ret.substitute(args, &[]);

    // So a diagnostic inside this body says `T` rather than `T0`.
    unifier
        .set_param_names(signature.generics.iter().map(|g| ast.name_of(*g).to_owned()).collect());
    unifier.set_region_param_names(
        signature.regions.iter().map(|g| ast.name_of(*g).to_owned()).collect(),
    );
    // Every `borrow` block in this body gets its name here as it is entered;
    // the list is indexed by block id and never shrinks, so a diagnostic can
    // still name a region whose block has closed -- which is exactly the
    // case an escape diagnostic has to talk about.
    unifier.set_region_block_names(Vec::new());

    let mut f = FnLowering {
        ast,
        signatures,
        externs,
        defs,
        unifier,
        mono,
        generic_names: signature.generics.clone(),
        generics: args.to_vec(),
        scopes: vec![Vec::new()],
        slots: Vec::new(),
        performed: Effects::pure(),
        region_params: signature
            .regions
            .iter()
            .enumerate()
            .map(|(i, name)| (*name, Region::Param(i as u32)))
            .collect(),
        region_outlives: signature.outlives.clone(),
        blocks: Vec::new(),
        open_blocks: Vec::new(),
        arenas: Vec::new(),
        slot_scope: Vec::new(),
        slot_origin: Vec::new(),
        ret: ret.clone(),
        trace: Trace::new(),
    };

    for (param, ty) in decl.params.iter().zip(params.iter()) {
        // Parameters are immutable: the shape of a binding handed to you, not
        // one you own outright. A `res` parameter is live from entry, and the
        // body owes exactly one consumption of it on every path.
        f.declare(param.name, ty.clone(), false, ast.type_span(param.ty));
    }
    let mut body = f.block(&decl.body)?;
    let mut slots = f.slots.clone();
    let escapes = f.escaped_slot();
    let performed = f.performed.clone();
    let trace = f.trace.finish();

    // §5 rule 4, over every binding rather than only the ones that return: a
    // slot's type may name a `borrow` block only if that block was open when
    // the slot was declared. One traversal of one type per slot, which is
    // what "escape is an occurs-check" buys.
    if let Some((name, region, span)) = escapes {
        return Err(Diagnostic::new(
            format!(
                "`{name}` would hold a reference into `{region}`, which is a `borrow` block it outlives"
            ),
            span,
        ));
    }

    settle_types(&mut body, unifier);
    for slot in slots.iter_mut() {
        *slot = unifier.resolve(slot);
    }

    // A slot whose type never got settled means the program did not say
    // enough. Better to name it here than to hand the backend a type that is
    // still a question.
    if let Some(unsettled) = slots.iter().find(|ty| ty.has_var()) {
        let _ = unsettled;
        return Err(Diagnostic::new(
            format!(
                "cannot tell what type a binding in `{}` has; add an annotation",
                ast.name_of(decl.name)
            ),
            ast.item_span(ast::ItemId(signature.item as u32)),
        ));
    }

    // Linearity runs last, on settled types: a mode is a fact about a type,
    // and a type is not a fact until inference is done (`linear.rs`).
    //
    // A generic body is checked once with its parameters rigid, where a
    // parameter is `val` (§3 — mode is never inferred), and then again per
    // instantiation, where it is whatever it was instantiated at. So mode
    // polymorphism does fall out of monomorphisation, at the price §12
    // warned about: a body that leaks its `T` is refused when someone
    // instantiates it at a `res` type, not where it is written. The
    // instantiation is named so the message says which one.
    if let Err(error) = linear::check(defs, unifier, &slots, &trace) {
        if args.is_empty() {
            return Err(error);
        }
        let at: Vec<String> = args.iter().map(|a| unifier.display(a)).collect();
        return Err(Diagnostic::new(
            format!(
                "{} (checking `{}` instantiated at `{}`)",
                error.message,
                ast.name_of(decl.name),
                at.join("`, `")
            ),
            error.span,
        ));
    }

    // §8.2: a row lists the capabilities a function *borrows*. Owning one
    // is strictly stronger and strictly more visible -- it is right there in
    // the parameter list -- so an effect the function has the authority for
    // outright does not appear in its row. That is why `main(world: World)`
    // has row `[]` while printing: it owns the authority rather than
    // borrowing it.
    //
    // Taken from the signature, not from the body, so the rule lives at the
    // boundary with every other rule.
    let mut performed = performed;
    let mut authority = Effects::pure();
    for param in &params {
        authority.union(&discharged_by(defs, param));
    }
    performed.discharge(&authority);

    // §7.3: the row is exact, or it is decoration. Both directions are
    // errors, and the over-wide one is not a warning -- an inexact row means
    // `[]` no longer means pure, which costs examples-as-tests and costs a
    // signature hash that means anything.
    let declared = &signature.effects;
    if let Some(label) = performed.missing_from(declared) {
        return Err(Diagnostic::new(
            format!(
                "`{}` performs `{label}`, which its row {declared} does not declare; narrow the body or widen the row",
                ast.name_of(decl.name)
            ),
            ast.item_span(ast::ItemId(signature.item as u32)),
        ));
    }
    if let Some(label) = declared.missing_from(&performed) {
        return Err(Diagnostic::new(
            format!(
                "`{}` declares `{label}` but never performs it; a row is exact or it is decoration",
                ast.name_of(decl.name)
            ),
            ast.item_span(ast::ItemId(signature.item as u32)),
        ));
    }

    if !terminates(&body) {
        return Err(Diagnostic::new(
            format!("function `{}` can finish without returning a value", ast.name_of(decl.name)),
            ast.item_span(ast::ItemId(signature.item as u32)),
        ));
    }

    Ok(Func {
        name: instance_name(ast.name_of(decl.name), args, unifier),
        effects: signature.effects.clone(),
        n_params: decl.params.len() as u32,
        slots,
        ret,
        body,
    })
}

/// A declaration's region parameters must be distinct and must not collide
/// with its type parameters, returning the scope a type in the signature is
/// resolved against.
///
/// Regions and types live in separate namespaces as far as the checker is
/// concerned -- one can never be written where the other is expected -- but
/// letting `fn f[T, &T]` through would make every diagnostic about it a
/// riddle, so it is refused.
fn check_region_names(
    ast: &Ast,
    regions: &[Symbol],
    generics: &[Symbol],
    span: Span,
) -> Result<Vec<(Symbol, Region)>, Diagnostic> {
    let mut scope = Vec::with_capacity(regions.len());
    for (index, name) in regions.iter().enumerate() {
        let text = ast.name_of(*name);
        if scope.iter().any(|(seen, _)| seen == name) {
            return Err(Diagnostic::new(
                format!("region parameter `{text}` is declared twice"),
                span,
            ));
        }
        if generics.contains(name) {
            return Err(Diagnostic::new(
                format!("`{text}` is both a type parameter and a region parameter here"),
                span,
            ));
        }
        scope.push((*name, Region::Param(index as u32)));
    }
    Ok(scope)
}

/// A declaration's type parameters must be distinct and must not shadow a
/// built-in type name.
fn check_generic_names(ast: &Ast, generics: &[Symbol], span: Span) -> Result<(), Diagnostic> {
    let mut seen: Vec<Symbol> = Vec::new();
    for name in generics {
        let text = ast.name_of(*name);
        if matches!(text, "int" | "bool") {
            return Err(Diagnostic::new(
                format!("`{text}` is a built-in type and cannot be a type parameter"),
                span,
            ));
        }
        if seen.contains(name) {
            return Err(Diagnostic::new(
                format!("type parameter `{text}` is declared twice"),
                span,
            ));
        }
        seen.push(*name);
    }
    Ok(())
}

/// Turn a written type into a real one.
///
/// `generics` is the enclosing declaration's type parameters; a written name
/// matching one of them is that parameter rather than a lookup. Parameters
/// shadow nothing else, because a declaration that named one `int` was already
/// refused.
fn resolve_type(
    ast: &Ast,
    defs: &[TypeDef],
    generics: &[Symbol],
    regions: &[(Symbol, Region)],
    id: TypeId,
) -> Result<Type, Diagnostic> {
    // Sized by default: `[T]` is a referent, and the one caller that may
    // have one is the reference that points at it.
    resolve_type_at(ast, defs, generics, regions, id, false)
}

fn resolve_type_at(
    ast: &Ast,
    defs: &[TypeDef],
    generics: &[Symbol],
    regions: &[(Symbol, Region)],
    id: TypeId,
    unsized_ok: bool,
) -> Result<Type, Diagnostic> {
    let span = ast.type_span(id);

    // `&r T`: the region must already be in scope. A name that is not a
    // region parameter of this declaration and not a `borrow` block open
    // around this type is simply not a region, which is the first half of
    // §5's escape example -- `fn escape(f: File) -> &r File` names an `r`
    // that exists nowhere.
    if let TypeExpr::Ref { unique, region, inner } = ast.ty(id) {
        let text = ast.name_of(*region);
        let Some((_, found)) = regions.iter().rev().find(|(name, _)| name == region) else {
            return Err(Diagnostic::new(
                format!(
                    "`{text}` is not a region in scope; a region comes from a `[&{text}]` parameter or a `borrow` block"
                ),
                span,
            ));
        };
        return Ok(Type::Ref {
            unique: *unique,
            region: *found,
            // The one place an unsized referent is allowed: `&r [T]` is how
            // a slice is written, and the reference is what gives it a size.
            inner: Box::new(resolve_type_at(ast, defs, generics, regions, *inner, true)?),
        });
    }

    // `Ffi("libc")`: a literal stands where a type argument does, because
    // what a capability is narrowed to is part of its type (§7.4). It names
    // nothing to look up and has no arguments of its own.
    if let TypeExpr::Lit(text) = ast.ty(id) {
        return Ok(Type::Lit(text.clone()));
    }

    // `[T]`: a shape rather than a name, so there is nothing to look up.
    // It is unsized, which is why it only ever appears under a reference --
    // and that is checked where a type is *used*, not here.
    if let TypeExpr::Slice(inner) = ast.ty(id) {
        // `[T]` has no size of its own -- that is what the length in a
        // slice is for -- so it cannot be a parameter, a field, a return
        // type or a type argument. Only a reference may point at one.
        if !unsized_ok {
            return Err(Diagnostic::new(
                "`[T]` has no size of its own, so it cannot be used as a value; write `&r [T]` or `&!r [T]`, which is a slice",
                span,
            ));
        }
        let element = resolve_type(ast, defs, generics, regions, *inner)?;
        return Ok(Type::Slice(Box::new(element)));
    }

    let TypeExpr::Name { name: written_name, args: written_args } = ast.ty(id) else {
        unreachable!("a reference, a literal and a slice were handled above");
    };
    let (written_name, written_args) = (*written_name, written_args.clone());
    let name = ast.name_of(written_name);

    let args = written_args
        .iter()
        .map(|arg| resolve_type(ast, defs, generics, regions, *arg))
        .collect::<Result<Vec<_>, _>>()?;

    if let Some(index) = generics.iter().position(|g| *g == written_name) {
        if !args.is_empty() {
            return Err(Diagnostic::new(
                format!("type parameter `{name}` takes no type arguments"),
                span,
            ));
        }
        return Ok(Type::Param(index as u32));
    }

    let (ty, arity) = match name {
        "int" => (Type::Int, 0),
        "bool" => (Type::Bool, 0),
        other => match defs.iter().find(|d| d.name == written_name) {
            Some(def) => (Type::Named(def.def, args.clone()), def.generics.len()),
            None => return Err(Diagnostic::new(format!("unknown type `{other}`"), span)),
        },
    };

    if args.len() != arity {
        return Err(Diagnostic::new(
            if arity == 0 {
                format!("`{name}` takes no type arguments")
            } else {
                format!(
                    "`{name}` takes {arity} type argument{}, but {} {} given",
                    if arity == 1 { "" } else { "s" },
                    args.len(),
                    if args.len() == 1 { "was" } else { "were" }
                )
            },
            span,
        ));
    }
    Ok(ty)
}

/// One `borrow` block, and the block it sits inside.
///
/// The parent link is all §5.2 needs: "`r_inner <= r_outer` holds exactly
/// when `r_outer`'s block lexically encloses `r_inner`'s", which is a walk up
/// this chain. O(depth), no fixpoint, and total because a chain has an end.
struct BorrowBlock {
    name: Symbol,
    parent: Option<u32>,
}

struct Binding {
    name: Symbol,
    slot: Slot,
    ty: Type,
    mutable: bool,
}

struct FnLowering<'a> {
    ast: &'a Ast,
    signatures: &'a [Signature],
    externs: &'a [ExternFn],
    defs: &'a [TypeDef],
    unifier: &'a mut Unifier,
    mono: &'a mut Mono,
    /// The names of this function's type parameters, so a written type can
    /// resolve to `Type::Param`...
    generic_names: Vec<Symbol>,
    /// ...and what each was instantiated at, so it can then be substituted.
    generics: Vec<Type>,
    scopes: Vec<Vec<Binding>>,
    slots: Vec<Type>,
    /// Every effect the body performs, unioned as the walk finds calls
    /// (§7.2: "inside a body there is nothing to infer but a union over the
    /// calls, which is a fold").
    performed: Effects,
    /// The region parameters of this function, so a written `&r` in the body
    /// resolves to the same `Region::Param` the signature used...
    region_params: Vec<(Symbol, Region)>,
    /// ...and the declared `a <= b` pairs, which the body may assume.
    region_outlives: Vec<(u32, u32)>,
    /// Every `borrow` block in this body, in the order they were entered.
    /// `Region::Block(i)` is an index here, so two *sibling* blocks are two
    /// different regions even though they nest to the same depth -- which
    /// depth alone could not tell apart, and §5.2's sibling case is exactly
    /// that.
    blocks: Vec<BorrowBlock>,
    /// The ids of the blocks open right now, outermost first.
    open_blocks: Vec<u32>,
    /// The block ids that are *arenas* (§6), in the order they opened. A
    /// block's position here is the number an `alloc` inside it carries to
    /// the backend, and membership is what tells an arena from a borrow's
    /// region -- `alloc[r]` into a borrow's region has nothing to allocate.
    arenas: Vec<u32>,
    /// For each slot, the innermost `borrow` block open when it was declared.
    /// A slot's type may mention that block and its ancestors, and nothing
    /// else: that is §5 rule 4, the escape occurs-check.
    slot_scope: Vec<Option<u32>>,
    /// The name and span each slot was declared with, for that check's
    /// diagnostic. A slot the backend made for itself has no name.
    slot_origin: Vec<(Option<Symbol>, Span)>,
    ret: Type,
    /// What the linearity checker replays once the types are settled
    /// (`linear.rs`). Recorded here because this is where the spans are.
    trace: Trace,
}

impl<'a> FnLowering<'a> {
    fn declare(&mut self, name: Symbol, ty: Type, mutable: bool, span: Span) -> Slot {
        let slot = self.temp(ty.clone());
        self.slot_origin[slot.0 as usize] = (Some(name), span);
        self.scopes.last_mut().expect("a scope is always open").push(Binding {
            name,
            slot,
            ty,
            mutable,
        });
        self.trace.emit(Event::Declare { slot, name: self.ast.name_of(name).to_owned(), span });
        slot
    }

    /// A slot with no name: somewhere for a value to live while its parts are
    /// taken out of it. It carries no linearity obligation of its own, because
    /// consuming the value it was built from already discharged one.
    fn temp(&mut self, ty: Type) -> Slot {
        let slot = Slot(self.slots.len() as u32);
        self.slots.push(ty);
        self.slot_scope.push(self.open_blocks.last().copied());
        self.slot_origin.push((None, Span::new(0, 0)));
        slot
    }

    fn lookup(&self, name: Symbol) -> Option<&Binding> {
        self.scopes.iter().rev().find_map(|scope| scope.iter().rev().find(|b| b.name == name))
    }

    fn declared_in_current_scope(&self, name: Symbol) -> bool {
        self.scopes.last().is_some_and(|scope| scope.iter().any(|b| b.name == name))
    }

    /// A type as written inside this body: resolved against the function's own
    /// type parameters, then substituted with what they were instantiated at.
    ///
    /// Regions in scope are the function's own parameters plus every
    /// `borrow` block open here, innermost last so an inner block shadows an
    /// outer one of the same name.
    fn written_type(&self, id: TypeId) -> Result<Type, Diagnostic> {
        let mut regions = self.region_params.clone();
        for id in &self.open_blocks {
            regions.push((self.blocks[*id as usize].name, Region::Block(*id)));
        }
        let resolved = resolve_type(self.ast, self.defs, &self.generic_names, &regions, id)?;
        Ok(resolved.substitute(&self.generics, &[]))
    }

    /// `release(cap)` — end a capability (§8.2).
    ///
    /// Checked here rather than through a written signature because there is
    /// no one type to write: every capability a program can hold by value
    /// ends the same way, and which ones those are is `released_only`'s
    /// answer, not the unifier's. A `Split` is excluded on purpose — taking
    /// one apart is what it is for, so releasing one whole would be
    /// discarding the authority inside it by accident.
    fn release(&mut self, args: &[ExprId], span: Span) -> Result<(Expr, Type), Diagnostic> {
        let [capability] = args else {
            return Err(Diagnostic::new(
                format!("`release` takes 1 argument, but {} were given", args.len()),
                span,
            ));
        };
        let (value, found) = self.expr(*capability)?;
        let resolved = self.unifier.resolve(&found);
        let is_releasable = matches!(&resolved, Type::Named(def, _) if released_only(*def));
        if !is_releasable {
            return Err(Diagnostic::new(
                format!(
                    "`{}` is not a capability and so has nothing to release",
                    self.unifier.display(&resolved)
                ),
                self.ast.expr_span(*capability),
            ));
        }
        Ok((Expr::Call { callee: Callee::Builtin(Builtin::Release), args: vec![value] }, Type::Int))
    }

    /// `narrow(cap, "libc")` — attenuate a capability (§7.4).
    ///
    /// Narrowing is prefix extension, exactly as the document's own example
    /// has it (`Fs("/var")` becomes `Fs("/var/log/app")`). The unnarrowed
    /// root is the empty string, which is a prefix of everything, so the
    /// capability `split` hands out can still become any library — and a
    /// capability already narrowed to one can never become another.
    fn narrow(&mut self, args: &[ExprId], span: Span) -> Result<(Expr, Type), Diagnostic> {
        let [capability, literal] = args else {
            return Err(Diagnostic::new(
                format!("`narrow` takes 2 arguments, but {} were given", args.len()),
                span,
            ));
        };
        let AstExpr::Str(target) = self.ast.expr(*literal) else {
            return Err(Diagnostic::new(
                "`narrow` takes a literal, so the refinement can be checked where it is written",
                self.ast.expr_span(*literal),
            ));
        };
        let target = target.clone();

        let (value, found) = self.expr(*capability)?;
        let resolved = self.unifier.resolve(&found);
        // Narrowing consumes what it attenuates, which is what makes the
        // wider capability unreachable afterwards. A borrow is precisely the
        // promise to give it back, so there is nothing here to consume.
        if let Type::Ref { inner, .. } = &resolved {
            return Err(Diagnostic::new(
                format!(
                    "`{}` is borrowed, and narrowing consumes what it attenuates; narrow the capability itself, before lending it",
                    self.unifier.display(inner)
                ),
                span,
            ));
        }
        let Type::Named(def, args) = &resolved else {
            return Err(Diagnostic::new(
                format!(
                    "`{}` is not a capability and cannot be narrowed",
                    self.unifier.display(&resolved)
                ),
                span,
            ));
        };
        if def.0 as usize != PRELUDE_FFI {
            return Err(Diagnostic::new(
                format!(
                    "`{}` carries no value to narrow; `Ffi` is the capability that names one",
                    self.unifier.display(&resolved)
                ),
                span,
            ));
        }
        let Some(Type::Lit(current)) = args.first() else {
            return Err(Diagnostic::new(
                "cannot tell what this capability was narrowed to; add an annotation",
                span,
            ));
        };
        if !target.starts_with(current.as_str()) {
            return Err(Diagnostic::new(
                format!(
                    "`{current}` cannot be narrowed to `{target}`: a capability is attenuated, never widened, and a program must not be able to grant itself what it was not given"
                ),
                span,
            ));
        }
        if &target == current {
            return Err(Diagnostic::new(
                format!("this narrows `{current}` to itself, which grants nothing new"),
                span,
            ));
        }

        Ok((value, Type::Named(DefId(PRELUDE_FFI as u32), vec![Type::Lit(target)])))
    }

    /// The prelude's type ids, in the order `prelude_types` declared them.
    fn prelude(&self) -> Vec<DefId> {
        self.defs[..4].iter().map(|d| d.def).collect()
    }

    /// Does `outer` outlive `inner` (§5.2)?
    ///
    /// Three cases, and each is a lookup rather than a solve:
    ///
    /// * two blocks — the outer one has the smaller depth, because nesting is
    ///   a stack and a stack is a total order;
    /// * a block against a region parameter — the parameter was open before
    ///   the body started, so it outlives every block in it and no block
    ///   outlives it;
    /// * two parameters — whatever the declaration's `where` clauses say,
    ///   reflexively and transitively.
    ///
    /// O(depth), no fixpoint, and total.
    fn outlives(&self, outer: Region, inner: Region) -> bool {
        match (outer, inner) {
            (a, b) if a == b => true,
            (Region::Block(a), Region::Block(b)) => self.encloses(a, b),
            (Region::Param(_), Region::Block(_)) => true,
            (Region::Block(_), Region::Param(_)) => false,
            (Region::Param(a), Region::Param(b)) => {
                // Walk the declared pairs from `b` outwards. The set of
                // parameters is tiny and the visited set makes a cyclic
                // `where` terminate rather than being an error of its own.
                let mut stack = vec![b];
                let mut seen = vec![b];
                while let Some(current) = stack.pop() {
                    if current == a {
                        return true;
                    }
                    for (i, o) in &self.region_outlives {
                        if *i == current && !seen.contains(o) {
                            seen.push(*o);
                            stack.push(*o);
                        }
                    }
                }
                false
            }
            // A region variable reaching here means a call site left one
            // unsolved, which `expect_type` reports where it can say more.
            _ => false,
        }
    }

    /// Does block `outer` lexically enclose block `inner`?
    fn encloses(&self, outer: u32, inner: u32) -> bool {
        let mut current = Some(inner);
        while let Some(id) = current {
            if id == outer {
                return true;
            }
            current = self.blocks[id as usize].parent;
        }
        false
    }

    /// The first binding whose type mentions a region it outlives, if any.
    ///
    /// Run once the body is walked and the types are settled: a slot's type
    /// is fixed at its declaration, so the only way it can name a block is if
    /// inference put it there.
    fn escaped_slot(&self) -> Option<(String, String, Span)> {
        for (index, ty) in self.slots.iter().enumerate() {
            let mut mentioned = Vec::new();
            self.unifier.resolve(ty).regions_into(&mut mentioned);
            for region in mentioned {
                let Region::Block(id) = region else { continue };
                if self.in_scope(region, self.slot_scope[index]) {
                    continue;
                }
                let (name, span) = self.slot_origin[index];
                let name =
                    name.map_or_else(|| "a value".to_owned(), |n| self.ast.name_of(n).to_owned());
                return Some((
                    name,
                    self.ast.name_of(self.blocks[id as usize].name).to_owned(),
                    span,
                ));
            }
        }
        None
    }

    /// Is `region` nameable from inside `scope`, the innermost block open
    /// where a slot was declared?
    fn in_scope(&self, region: Region, scope: Option<u32>) -> bool {
        match region {
            Region::Block(id) => scope.is_some_and(|inner| self.encloses(id, inner)),
            // A region parameter is open for the whole body.
            _ => true,
        }
    }

    /// Fresh inference variables, one per type parameter of a declaration.
    fn fresh_args(&mut self, count: usize) -> Vec<Type> {
        (0..count).map(|_| self.unifier.fresh()).collect()
    }

    /// Require `found` to be usable where `expected` is wanted, reporting the
    /// failure at `span`.
    ///
    /// Equality, with two coercions on top and no others.
    ///
    /// §5.2's: a reference whose region *outlives* the expected one is
    /// accepted, because it is valid for at least as long as it needs to be.
    ///
    /// And §6's: a unique reference is accepted where a shared one is
    /// expected, never the reverse. `&!r T` is `&r T` plus permission to
    /// write, so handing one over as read-only gives the callee strictly
    /// less than it already had. Without this, arena data would be
    /// unreachable from every helper written against `&r` — §6 hands back
    /// `&!a` and nothing else, so `value_of[&r](n: &r Node)` could never be
    /// called on anything allocated. The referent stays invariant: "`T`
    /// never changes" is untouched.
    fn expect_type(&mut self, expected: &Type, found: &Type, span: Span) -> Result<(), Diagnostic> {
        let want = self.unifier.shallow(expected);
        let got = self.unifier.shallow(found);
        if let (
            Type::Ref { unique: want_unique, region: want_region, inner: want_inner },
            Type::Ref { unique: got_unique, region: got_region, inner: got_inner },
        ) = (&want, &got)
            && (want_unique == got_unique || (!*want_unique && *got_unique))
        {
            let wanted = self.unifier.resolve_region(*want_region);
            let given = self.unifier.resolve_region(*got_region);
            if wanted.is_var() || given.is_var() {
                // One side is a call site's fresh region: there is nothing to
                // compare yet, so solve it. That is §5.1's instantiation.
                self.unify_regions_at(wanted, given, span)?;
            } else if !self.outlives(given, wanted) {
                return Err(Diagnostic::new(
                    format!(
                        "`{}` does not outlive `{}`, so a reference valid for the first cannot be used where the second is expected",
                        self.unifier.display_region(given),
                        self.unifier.display_region(wanted)
                    ),
                    span,
                ));
            }
            let (want_inner, got_inner) = (want_inner.clone(), got_inner.clone());
            return self.expect_exact(&want_inner, &got_inner, span);
        }
        self.expect_exact(&want, &got, span)
    }

    fn unify_regions_at(
        &mut self,
        expected: Region,
        found: Region,
        span: Span,
    ) -> Result<(), Diagnostic> {
        match self.unifier.unify_regions(expected, found) {
            Ok(()) => Ok(()),
            Err(_) => Err(Diagnostic::new(
                format!(
                    "`{}` and `{}` are different regions",
                    self.unifier.display_region(expected),
                    self.unifier.display_region(found)
                ),
                span,
            )),
        }
    }

    /// Plain equality, with no coercion anywhere inside.
    fn expect_exact(
        &mut self,
        expected: &Type,
        found: &Type,
        span: Span,
    ) -> Result<(), Diagnostic> {
        match self.unifier.unify(expected, found) {
            Ok(()) => Ok(()),
            Err(UnifyError::Mismatch { expected, found }) => Err(Diagnostic::new(
                format!(
                    "expected `{}`, found `{}`",
                    self.unifier.display(&expected),
                    self.unifier.display(&found)
                ),
                span,
            )),
            Err(UnifyError::Infinite { ty, .. }) => Err(Diagnostic::new(
                format!("this would build an infinite type, `{}`", self.unifier.display(&ty)),
                span,
            )),
            // Two references from different `borrow` blocks, neither of which
            // encloses the other: §5.2's sibling case.
            Err(UnifyError::Regions { expected, found }) => Err(Diagnostic::new(
                format!(
                    "`{}` and `{}` are different regions, and neither outlives the other",
                    self.unifier.display_region(expected),
                    self.unifier.display_region(found)
                ),
                span,
            )),
            Err(UnifyError::Uniqueness { expected }) => Err(Diagnostic::new(
                if expected {
                    "expected a unique reference `&!`, found a shared one `&`"
                } else {
                    "expected a shared reference `&`, found a unique one `&!`"
                },
                span,
            )),
        }
    }

    fn block(&mut self, block: &Block) -> Result<Vec<Stmt>, Diagnostic> {
        self.scopes.push(Vec::new());
        self.trace.open();
        let out = self.stmts(&block.stmts);
        let events = self.trace.close();
        self.trace.emit(Event::Scope(events));
        self.scopes.pop();
        out
    }

    fn stmts(&mut self, ids: &[StmtId]) -> Result<Vec<Stmt>, Diagnostic> {
        let mut out: Vec<Stmt> = Vec::new();
        for (i, &id) in ids.iter().enumerate() {
            if i > 0 && terminates(&out) {
                return Err(Diagnostic::new(
                    "this statement is unreachable",
                    self.ast.stmt_span(id),
                ));
            }
            out.extend(self.stmt(id)?);
        }
        Ok(out)
    }

    /// One source statement, which may lower to more than one IR statement:
    /// destructuring binds each part separately.
    fn stmt(&mut self, id: StmtId) -> Result<Vec<Stmt>, Diagnostic> {
        if let AstStmt::Destructure { .. } = self.ast.stmt(id) {
            return self.destructure(id);
        }
        Ok(vec![self.simple_stmt(id)?])
    }

    fn simple_stmt(&mut self, id: StmtId) -> Result<Stmt, Diagnostic> {
        let span = self.ast.stmt_span(id);
        Ok(match self.ast.stmt(id) {
            AstStmt::Let { name, mutable, ty, value } => {
                // The initialiser is resolved *before* the binding exists, so
                // `let x = x;` reads the outer `x` or fails, and never itself.
                let (value, found) = self.expr(*value)?;
                let declared = match ty {
                    Some(written) => {
                        let declared = self.written_type(*written)?;
                        self.expect_type(
                            &declared,
                            &found,
                            self.ast.expr_span(match self.ast.stmt(id) {
                                AstStmt::Let { value, .. } => *value,
                                _ => unreachable!(),
                            }),
                        )?;
                        declared
                    }
                    None => found,
                };
                if self.declared_in_current_scope(*name) {
                    return Err(Diagnostic::new(
                        format!(
                            "`{}` is already bound in this block (shadowing is only allowed in an inner block)",
                            self.ast.name_of(*name)
                        ),
                        span,
                    ));
                }
                let slot = self.declare(*name, declared, *mutable, span);
                Stmt::Store { place: Place::Slot(slot), value }
            }
            AstStmt::Assign { place, value } => {
                let value_span = self.ast.expr_span(*value);
                let (value, found) = self.expr(*value)?;
                // The place is resolved *after* the value, so `x = x + 1`
                // reads `x` before the write is recorded.
                let (place, declared) = self.place(*place, span)?;
                self.expect_type(&declared, &found, value_span)?;
                Stmt::Store { place, value }
            }
            AstStmt::Expr(e) => {
                let (value, found) = self.expr(*e)?;
                self.trace.emit(Event::Discard {
                    ty: found,
                    what: "this value",
                    span: self.ast.expr_span(*e),
                });
                Stmt::Eval(value)
            }
            AstStmt::If { cond, then_block, else_block } => {
                let cond = self.condition(*cond)?;
                self.trace.open();
                let then_body = self.block(then_block)?;
                let then_events = self.trace.close();
                self.trace.open();
                let else_body = match else_block {
                    Some(block) => self.block(block)?,
                    None => Vec::new(),
                };
                let else_events = self.trace.close();
                // An `if` with no `else` still has two arms; the missing one
                // is empty, which is exactly what makes a lone `if` that
                // consumes a value a disagreement (§4.2).
                self.trace.emit(Event::Branch { arms: vec![then_events, else_events], span });
                Stmt::If { cond, then_body, else_body }
            }
            AstStmt::While { cond, body } => {
                // The condition is evaluated before every iteration, so it
                // belongs to the body as far as the back edge is concerned.
                self.trace.open();
                let cond = self.condition(*cond)?;
                let body = self.block(body)?;
                let events = self.trace.close();
                self.trace.emit(Event::Loop { body: events, span });
                Stmt::While { cond, body }
            }
            AstStmt::Match { scrutinee, arms } => self.match_stmt(*scrutinee, arms, span)?,
            AstStmt::Borrow { value, unique, region, body } => {
                self.borrow_stmt(*value, *unique, *region, body, span)?
            }
            AstStmt::Region { region, body } => self.region_stmt(*region, body)?,
            AstStmt::Return(e) => {
                let (value, found) = self.expr(*e)?;
                // §5 rule 4, at the one place a value can leave a region: a
                // return type names only the function's own region
                // parameters, so a reference into a `borrow` block here is an
                // escape. Checked before the types are compared, because
                // "`r` does not outlive `q`" is a worse way to say it.
                let mut mentioned = Vec::new();
                self.unifier.resolve(&found).regions_into(&mut mentioned);
                if let Some(Region::Block(id)) =
                    mentioned.into_iter().find(|r| matches!(r, Region::Block(_)))
                {
                    let kind =
                        if self.arenas.contains(&id) { "an arena" } else { "a `borrow` block" };
                    return Err(Diagnostic::new(
                        format!(
                            "this returns a reference into `{}`, which is {kind} in this function; a reference may not outlive its region",
                            self.ast.name_of(self.blocks[id as usize].name)
                        ),
                        self.ast.expr_span(*e),
                    ));
                }
                let ret = self.ret.clone();
                self.expect_type(&ret, &found, self.ast.expr_span(*e))?;
                self.trace.emit(Event::Return { span });
                Stmt::Return(value)
            }
            AstStmt::Destructure { .. } => unreachable!("handled before the match"),
        })
    }

    /// `let File { fd } = f;` — §4.1's third consumer.
    ///
    /// The whole is spent and the parts are produced, each subject to the
    /// rule in turn. Without it a `res` value could never be destroyed: there
    /// is no `drop`, and a type whose parts are all `val` is exactly where an
    /// obligation ends.
    ///
    /// The value is evaluated once into an unnamed slot, so `let P { a, b } =
    /// make();` calls `make` once however many fields it has.
    fn destructure(&mut self, id: StmtId) -> Result<Vec<Stmt>, Diagnostic> {
        let span = self.ast.stmt_span(id);
        let AstStmt::Destructure { struct_name, fields, value } = self.ast.stmt(id) else {
            unreachable!("only called for a destructuring `let`");
        };
        let (struct_name, fields, value_id) = (*struct_name, fields.clone(), *value);
        let value_span = self.ast.expr_span(value_id);
        let (value, found) = self.expr(value_id)?;

        let text = self.ast.name_of(struct_name);
        let Some(def) = self.defs.iter().find(|d| d.name == struct_name) else {
            return Err(Diagnostic::new(format!("`{text}` is not a struct"), span));
        };
        let (def_id, generic_count) = (def.def, def.generics.len());
        if released_only(def_id) {
            return Err(Diagnostic::new(
                format!(
                    "`{text}` is a capability and is destroyed by `release`, not by being taken apart; authority is a resource and the function that ends one is named"
                ),
                span,
            ));
        }
        let DefKind::Struct(declared) = &def.kind else {
            return Err(Diagnostic::new(
                format!("`{text}` is an enum, not a struct; take it apart with `match`"),
                span,
            ));
        };
        let declared = declared.clone();

        let type_args = self.fresh_args(generic_count);
        self.expect_type(&Type::Named(def_id, type_args.clone()), &found, value_span)?;

        if fields.len() != declared.len() {
            return Err(Diagnostic::new(
                format!(
                    "`{text}` has {} field{}, but this pattern names {}; destructuring takes the whole value apart",
                    declared.len(),
                    if declared.len() == 1 { "" } else { "s" },
                    fields.len()
                ),
                span,
            ));
        }

        let mut order: Vec<usize> = Vec::with_capacity(fields.len());
        for (position, field) in fields.iter().enumerate() {
            let field_text = self.ast.name_of(*field);
            let Some(index) = declared.iter().position(|(n, _)| n == field) else {
                return Err(Diagnostic::new(format!("`{text}` has no field `{field_text}`"), span));
            };
            if order.contains(&index) {
                return Err(Diagnostic::new(format!("field `{field_text}` is named twice"), span));
            }
            if self.declared_in_current_scope(*field) {
                return Err(Diagnostic::new(
                    format!(
                        "`{field_text}` is already bound in this block (shadowing is only allowed in an inner block)"
                    ),
                    span,
                ));
            }
            if fields[..position].contains(field) {
                return Err(Diagnostic::new(
                    format!("`{field_text}` is bound twice in this pattern"),
                    span,
                ));
            }
            order.push(index);
        }

        let whole = self.temp(Type::Named(def_id, type_args.clone()));
        let mut out = vec![Stmt::Store { place: Place::Slot(whole), value }];
        for (field, index) in fields.iter().zip(order) {
            let ty = declared[index].1.substitute(&type_args, &[]);
            let slot = self.declare(*field, ty, false, span);
            out.push(Stmt::Store {
                place: Place::Slot(slot),
                value: Expr::Field {
                    base: Box::new(Expr::Load(whole)),
                    def: def_id,
                    args: type_args.clone(),
                    index: index as u32,
                },
            });
        }
        Ok(out)
    }

    /// `borrow x as &r in { .. }` (§5).
    ///
    /// Freezes `x` for the block and binds a reference to it. The region and
    /// the reference share the name `r`, which is how §5 writes it: `r` is
    /// the region in a type and the reference in an expression, and the two
    /// namespaces never meet.
    /// Resolve the left side of an assignment to a place (§5 rule 2).
    ///
    /// A place is a whole local or a field of whatever a *unique* reference
    /// points at, and nothing else. Two things it deliberately is not:
    ///
    /// * a field of an owned local. That is a partial write, and what one
    ///   means for a binding holding a `res` field is a question §4 does not
    ///   answer. Writing the whole value says the same thing and asks
    ///   nothing new.
    /// * anything reached through a *shared* reference. Freezing promises
    ///   the referent will not change; writing through it is that promise
    ///   broken, whoever holds the reference.
    fn place(&mut self, id: ExprId, span: Span) -> Result<(Place, Type), Diagnostic> {
        match self.ast.expr(id) {
            AstExpr::Name(name) => {
                let text = self.ast.name_of(*name);
                let Some(binding) = self.lookup(*name) else {
                    return Err(Diagnostic::new(format!("`{text}` is not bound here"), span));
                };
                if !binding.mutable {
                    return Err(Diagnostic::new(
                        format!("`{text}` is immutable; declare it with `var` to assign to it"),
                        span,
                    ));
                }
                let (slot, ty) = (binding.slot, binding.ty.clone());
                self.trace.emit(Event::Assign { slot, span });
                Ok((Place::Slot(slot), ty))
            }
            AstExpr::Index { base, index } => {
                let (base_id, index_id) = (*base, *index);
                let base_span = self.ast.expr_span(base_id);
                let (base_expr, base_ty) = self.expr(base_id)?;
                let resolved = self.unifier.resolve(&base_ty);
                // A shared slice promises its elements will not change, the
                // same promise a shared reference makes about its referent.
                if let Type::Ref { unique: false, .. } = &resolved {
                    return Err(Diagnostic::new(
                        "this is a shared slice `&`, which promises its elements will not change; `alloc_slice` hands back a unique one"
                            .to_owned(),
                        base_span,
                    ));
                }
                let element = self.element_of(&base_ty, base_span)?;
                let index_span = self.ast.expr_span(index_id);
                let (index_expr, index_ty) = self.expr(index_id)?;
                self.expect_type(&Type::Int, &index_ty, index_span)?;
                Ok((
                    Place::Element { base: base_expr, index: index_expr, element: element.clone() },
                    element,
                ))
            }
            AstExpr::Field { base, name } => {
                let (base_id, field) = (*base, *name);
                let base_span = self.ast.expr_span(base_id);
                let (base_expr, base_ty) = self.expr(base_id)?;
                let resolved = self.unifier.resolve(&base_ty);
                let Type::Ref { unique, inner, .. } = resolved else {
                    return Err(Diagnostic::new(
                        format!(
                            "`{}` is not a reference, so this would write one field and leave the rest; assign the whole value instead",
                            self.unifier.display(&resolved)
                        ),
                        base_span,
                    ));
                };
                if !unique {
                    return Err(Diagnostic::new(
                        "this is a shared reference `&`, which promises its referent will not change; `borrow mut` binds one that may be written through"
                            .to_owned(),
                        base_span,
                    ));
                }
                let Type::Named(def_id, type_args) = self.unifier.resolve(&inner) else {
                    return Err(Diagnostic::new(
                        format!("`{}` has no fields", self.unifier.display(&inner)),
                        base_span,
                    ));
                };
                let def =
                    self.defs.iter().find(|d| d.def == def_id).expect("a named type is declared");
                let field_text = self.ast.name_of(field);
                let DefKind::Struct(fields) = &def.kind else {
                    return Err(Diagnostic::new(
                        format!(
                            "`{}` is an enum; its payload is reached by matching, not with `.`",
                            self.ast.name_of(def.name)
                        ),
                        base_span,
                    ));
                };
                let Some(index) = fields.iter().position(|(n, _)| *n == field) else {
                    return Err(Diagnostic::new(
                        format!("`{}` has no field `{field_text}`", self.ast.name_of(def.name)),
                        span,
                    ));
                };
                let ty = fields[index].1.substitute(&type_args, &[]);
                // The write destroys whatever was there. For a `val` field
                // that costs nothing; for a `res` one it is the silent drop
                // §4 exists to forbid, and there is no consumer named here.
                self.trace.emit(Event::Discard {
                    ty: ty.clone(),
                    what: "the value overwritten here",
                    span,
                });
                Ok((
                    Place::Field {
                        base: base_expr,
                        def: def_id,
                        args: type_args,
                        index: index as u32,
                    },
                    ty,
                ))
            }
            _ => Err(Diagnostic::new(
                "this cannot be assigned to; a place is a binding or a field reached through a unique reference",
                span,
            )),
        }
    }

    fn borrow_stmt(
        &mut self,
        value: Symbol,
        unique: bool,
        region: Symbol,
        body: &Block,
        span: Span,
    ) -> Result<Stmt, Diagnostic> {
        let text = self.ast.name_of(value);
        let Some(binding) = self.lookup(value) else {
            return Err(Diagnostic::new(format!("`{text}` is not bound here"), span));
        };
        let (referent, referent_ty) = (binding.slot, binding.ty.clone());

        let id = self.blocks.len() as u32;
        self.blocks.push(BorrowBlock { name: region, parent: self.open_blocks.last().copied() });
        self.open_blocks.push(id);
        let names: Vec<String> =
            self.blocks.iter().map(|b| self.ast.name_of(b.name).to_owned()).collect();
        self.unifier.set_region_block_names(names);

        self.scopes.push(Vec::new());
        self.trace.open();
        // Frozen for the whole block: not movable, not consumable. A `val`
        // referent notices nothing, because reading one was never a move.
        self.trace.emit(Event::Freeze { slot: referent, unique, span });
        let reference =
            Type::Ref { unique, region: Region::Block(id), inner: Box::new(referent_ty) };
        let reference = self.declare(region, reference, false, span);
        let lowered = self.stmts(&body.stmts);
        self.trace.emit(Event::Thaw { slot: referent, unique });
        let events = self.trace.close();
        self.trace.emit(Event::Scope(events));
        self.scopes.pop();

        self.open_blocks.pop();
        Ok(Stmt::Borrow { referent, reference, unique, body: lowered? })
    }

    /// `region a { .. }` — an arena (§6).
    ///
    /// Almost exactly `borrow_stmt` with the referent taken out, and that is
    /// the section's whole claim made structural: the region it opens is a
    /// block in the same table, so §5.2's outlives relation, §5's
    /// occurs-check and the scope rules all apply to it without a line of
    /// new reasoning. What escapes an arena is decided by the code that
    /// decides what escapes a borrow, because there is only one.
    fn region_stmt(&mut self, region: Symbol, body: &Block) -> Result<Stmt, Diagnostic> {
        let id = self.blocks.len() as u32;
        self.blocks.push(BorrowBlock { name: region, parent: self.open_blocks.last().copied() });
        self.open_blocks.push(id);
        let names: Vec<String> =
            self.blocks.iter().map(|b| self.ast.name_of(b.name).to_owned()).collect();
        self.unifier.set_region_block_names(names);

        // The arena's number is its position among this function's arenas,
        // which is what an `alloc` inside it names at the backend.
        let arena = self.arenas.len() as u32;
        self.arenas.push(id);

        self.scopes.push(Vec::new());
        self.trace.open();
        let lowered = self.stmts(&body.stmts);
        let events = self.trace.close();
        self.trace.emit(Event::Scope(events));
        self.scopes.pop();

        self.open_blocks.pop();
        Ok(Stmt::Region { arena, body: lowered? })
    }

    /// `alloc[a](value)` (§6).
    ///
    /// Three questions, and the first two are lookups rather than solves:
    /// which arena `a` names, whether what is being allocated is `val`, and
    /// what type comes back.
    fn alloc(
        &mut self,
        region: Symbol,
        value: ExprId,
        span: Span,
    ) -> Result<(Expr, Type), Diagnostic> {
        // An arena is named, and the name has to be one that is open here.
        // A region *parameter* is not an arena: a caller's region is not
        // this function's to allocate in, and nothing was opened to hold it.
        let id = self.open_arena(region, span)?;

        let value_span = self.ast.expr_span(value);
        let (lowered, ty) = self.expr(value)?;
        let resolved = self.unifier.resolve(&ty);
        // §6.1: an arena releases memory. It does not close files, release
        // capabilities or run anything -- so a `res` value put in one would
        // have its memory reclaimed with its obligation undischarged, which
        // is a leak with a static blessing.
        if mode_of(self.defs, self.unifier, &resolved) == Mode::Res {
            return Err(Diagnostic::new(
                format!(
                    "`{}` is `res`, and an arena holds `val` data only: releasing one reclaims memory and runs nothing, so a linear obligation put inside would be dropped rather than discharged",
                    self.unifier.display(&resolved)
                ),
                value_span,
            ));
        }

        let reference = Type::Ref {
            unique: true,
            region: Region::Block(id),
            inner: Box::new(resolved.clone()),
        };
        Ok((
            Expr::Alloc { arena: self.arena_of(id), ty: resolved, value: Box::new(lowered) },
            reference,
        ))
    }

    /// `len(s)` — how many elements a slice has.
    ///
    /// The length travels in the slice itself, so this reads a value that
    /// is already there rather than computing one: a slice is a pointer and
    /// a length, and `len` is the second half.
    fn len(&mut self, args: &[ExprId], span: Span) -> Result<(Expr, Type), Diagnostic> {
        let [slice] = args else {
            return Err(Diagnostic::new(
                format!("`len` takes 1 argument, but {} were given", args.len()),
                span,
            ));
        };
        let slice_span = self.ast.expr_span(*slice);
        let (value, ty) = self.expr(*slice)?;
        self.element_of(&ty, slice_span)?;
        Ok((Expr::Len(Box::new(value)), Type::Int))
    }

    /// `alloc_slice[a](count, fill)` — a run of `count` copies of `fill`
    /// (§6, and `defined-behaviour.md` §8's bounds rule).
    ///
    /// The length is a runtime value, which is what makes this a slice
    /// rather than an array: an array's length lives in its type, and a
    /// length in a type is a second kind of generic parameter that M3 is
    /// not buying.
    fn alloc_slice(
        &mut self,
        region: Symbol,
        count: ExprId,
        fill: ExprId,
        span: Span,
    ) -> Result<(Expr, Type), Diagnostic> {
        let id = self.open_arena(region, span)?;

        let count_span = self.ast.expr_span(count);
        let (count_expr, count_ty) = self.expr(count)?;
        self.expect_type(&Type::Int, &count_ty, count_span)?;

        let fill_span = self.ast.expr_span(fill);
        let (fill_expr, element) = self.expr(fill)?;
        let element = self.unifier.resolve(&element);
        // §6.1 again, and for the same reason: an arena reclaims memory and
        // runs nothing, so a linear obligation inside would be dropped
        // rather than discharged. A slice makes it worse -- there would be
        // `count` of them -- but the rule is the one rule, not a new one.
        if mode_of(self.defs, self.unifier, &element) == Mode::Res {
            return Err(Diagnostic::new(
                format!(
                    "`{}` is `res`, and an arena holds `val` data only: the fill is copied into every element, and a linear value cannot be copied at all",
                    self.unifier.display(&element)
                ),
                fill_span,
            ));
        }

        let slice = Type::Slice(Box::new(element.clone()));
        Ok((
            Expr::AllocSlice {
                arena: self.arena_of(id),
                element,
                count: Box::new(count_expr),
                fill: Box::new(fill_expr),
            },
            Type::Ref { unique: true, region: Region::Block(id), inner: Box::new(slice) },
        ))
    }

    /// `s[i]` — one element, bounds-checked at runtime.
    ///
    /// The check is not optional and not a mode: an out-of-range index
    /// traps, because the alternative is reading past the end of an
    /// allocation, which is the undefined behaviour this language does not
    /// have (`defined-behaviour.md` §1).
    fn index(
        &mut self,
        base: ExprId,
        index: ExprId,
        span: Span,
    ) -> Result<(Expr, Type), Diagnostic> {
        let base_span = self.ast.expr_span(base);
        let (base_expr, base_ty) = self.expr(base)?;
        let element = self.element_of(&base_ty, base_span)?;

        let index_span = self.ast.expr_span(index);
        let (index_expr, index_ty) = self.expr(index)?;
        self.expect_type(&Type::Int, &index_ty, index_span)?;
        let _ = span;

        Ok((
            Expr::Index {
                base: Box::new(base_expr),
                index: Box::new(index_expr),
                element: element.clone(),
            },
            element,
        ))
    }

    /// The element type of whatever `ty` is, if it is a slice at all.
    fn element_of(&mut self, ty: &Type, span: Span) -> Result<Type, Diagnostic> {
        let resolved = self.unifier.resolve(ty);
        if let Type::Ref { inner, .. } = &resolved
            && let Type::Slice(element) = self.unifier.resolve(inner)
        {
            return Ok(self.unifier.resolve(&element));
        }
        Err(Diagnostic::new(
            format!(
                "`{}` is not a slice, so it cannot be indexed",
                self.unifier.display(&resolved)
            ),
            span,
        ))
    }

    /// The arena `region` names, if it is one open here.
    fn open_arena(&mut self, region: Symbol, span: Span) -> Result<u32, Diagnostic> {
        let text = self.ast.name_of(region);
        self.open_blocks
            .iter()
            .rev()
            .find(|id| self.arenas.contains(id) && self.blocks[**id as usize].name == region)
            .copied()
            .ok_or_else(|| {
                Diagnostic::new(
                    format!(
                        "`{text}` is not an arena open here; `alloc` allocates in a `region {text} {{ .. }}` block"
                    ),
                    span,
                )
            })
    }

    /// Which arena number a block id was given.
    fn arena_of(&self, block: u32) -> u32 {
        self.arenas.iter().position(|id| *id == block).expect("an arena block") as u32
    }

    /// Check a `match`: the scrutinee is an enum, every arm names a variant of
    /// it, no variant is matched twice, and between them the arms cover
    /// everything.
    ///
    /// Exhaustiveness is the point. A `match` that silently did nothing for an
    /// unlisted variant would be a hole in the type system exactly where the
    /// type system is supposed to pay for itself.
    fn match_stmt(
        &mut self,
        scrutinee: ExprId,
        arms: &[ast::MatchArm],
        span: Span,
    ) -> Result<Stmt, Diagnostic> {
        let scrutinee_span = self.ast.expr_span(scrutinee);
        let (value, scrutinee_ty) = self.expr(scrutinee)?;
        let resolved = self.unifier.resolve(&scrutinee_ty);

        let Type::Named(def_id, type_args) = resolved else {
            return Err(Diagnostic::new(
                format!(
                    "`{}` cannot be matched (M1 matches enums)",
                    self.unifier.display(&resolved)
                ),
                scrutinee_span,
            ));
        };
        let def = self.defs.iter().find(|d| d.def == def_id).expect("a declared type");
        let enum_name = self.ast.name_of(def.name).to_owned();
        let DefKind::Enum(variants) = &def.kind else {
            return Err(Diagnostic::new(
                format!("`{enum_name}` is a struct, not an enum; there is nothing to match on"),
                scrutinee_span,
            ));
        };
        let variants = variants.clone();

        let scrutinee_ty = Type::Named(def_id, type_args.clone());
        let mut covered = vec![false; variants.len()];
        let mut wildcard = false;
        let mut lowered: Vec<Arm> = Vec::new();
        let mut arm_events: Vec<Vec<Event>> = Vec::new();

        for arm in arms {
            if wildcard {
                return Err(Diagnostic::new(
                    "this arm is unreachable: `_` above it already matches everything",
                    span,
                ));
            }

            let (variant_index, bindings) = match &arm.pattern {
                ast::Pattern::Wildcard => {
                    if covered.iter().all(|c| *c) {
                        return Err(Diagnostic::new(
                            format!(
                                "this `_` is unreachable: every variant of `{enum_name}` is already matched"
                            ),
                            span,
                        ));
                    }
                    wildcard = true;
                    (None, Vec::new())
                }
                ast::Pattern::Variant { enum_name: written, variant, bindings } => {
                    let written_text = self.ast.name_of(*written);
                    if *written != def.name {
                        return Err(Diagnostic::new(
                            format!(
                                "expected a variant of `{enum_name}`, found one of `{written_text}`"
                            ),
                            span,
                        ));
                    }
                    let variant_text = self.ast.name_of(*variant);
                    let Some(index) = variants.iter().position(|(n, _)| n == variant) else {
                        return Err(Diagnostic::new(
                            format!("`{enum_name}` has no variant `{variant_text}`"),
                            span,
                        ));
                    };
                    if covered[index] {
                        return Err(Diagnostic::new(
                            format!("`{enum_name}::{variant_text}` is matched twice"),
                            span,
                        ));
                    }
                    let payload = &variants[index].1;
                    if bindings.len() != payload.len() {
                        return Err(Diagnostic::new(
                            format!(
                                "`{enum_name}::{variant_text}` carries {} value{}, but the pattern binds {}",
                                payload.len(),
                                if payload.len() == 1 { "" } else { "s" },
                                bindings.len()
                            ),
                            span,
                        ));
                    }
                    covered[index] = true;
                    (Some(index as u32), bindings.clone())
                }
            };

            // Each arm's bindings live in their own scope, so two arms may
            // bind the same name to different types.
            self.scopes.push(Vec::new());
            self.trace.open();
            let mut slots: Vec<Option<Slot>> = Vec::new();
            if variant_index.is_none() {
                // A `_` arm consumes the scrutinee and never names its parts.
                // For a `val` enum that is a discard and costs nothing; for a
                // `res` one it is the silent drop §4 exists to forbid.
                self.trace.emit(Event::Discard {
                    ty: scrutinee_ty.clone(),
                    what: "the value matched here",
                    span: scrutinee_span,
                });
            }
            if let Some(index) = variant_index {
                // A binding's type comes from the scrutinee's own type
                // arguments: matching `Option[int]` binds an `int`.
                let payload: Vec<Type> = variants[index as usize]
                    .1
                    .iter()
                    .map(|t| t.substitute(&type_args, &[]))
                    .collect();
                for (binding, ty) in bindings.iter().zip(payload) {
                    match binding {
                        Some(name) => {
                            if self.declared_in_current_scope(*name) {
                                self.scopes.pop();
                                self.trace.close();
                                return Err(Diagnostic::new(
                                    format!(
                                        "`{}` is bound twice in this pattern",
                                        self.ast.name_of(*name)
                                    ),
                                    span,
                                ));
                            }
                            slots.push(Some(self.declare(*name, ty, false, span)));
                        }
                        // `_` still occupies a payload position; it just has
                        // no name, so the backend drops the value — which is
                        // only allowed when there is nothing to drop.
                        None => {
                            self.trace.emit(Event::Discard { ty, what: "this payload", span });
                            slots.push(None);
                        }
                    }
                }
            }
            let body = self.stmts(&arm.body.stmts);
            let events = self.trace.close();
            arm_events.push(vec![Event::Scope(events)]);
            self.scopes.pop();
            lowered.push(Arm { variant: variant_index, bindings: slots, body: body? });
        }

        if !wildcard && !covered.iter().all(|c| *c) {
            let missing: Vec<String> = covered
                .iter()
                .enumerate()
                .filter(|(_, seen)| !**seen)
                .map(|(i, _)| format!("`{enum_name}::{}`", self.ast.name_of(variants[i].0)))
                .collect();
            return Err(Diagnostic::new(
                format!("this `match` does not cover {}", missing.join(", ")),
                span,
            ));
        }

        // A `match` is exhaustive by the check above, so its arms are the
        // whole branch: there is no implicit fall-through arm to join.
        self.trace.emit(Event::Branch { arms: arm_events, span });
        Ok(Stmt::Match { scrutinee: value, def: def_id, args: type_args, arms: lowered })
    }

    /// A condition is a `bool`. M0 tested "non-zero"; M1 has a type for the
    /// question, so the convention becomes a rule.
    fn condition(&mut self, id: ExprId) -> Result<Expr, Diagnostic> {
        let (expr, found) = self.expr(id)?;
        self.expect_type(&Type::Bool, &found, self.ast.expr_span(id))?;
        Ok(expr)
    }

    fn expr(&mut self, id: ExprId) -> Result<(Expr, Type), Diagnostic> {
        let span = self.ast.expr_span(id);
        Ok(match self.ast.expr(id) {
            AstExpr::Int(v) => (Expr::Int(*v), Type::Int),
            // A literal the checker reads and the program never holds.
            // `narrow` intercepts its own argument before it reaches here,
            // so anywhere else is a place a string cannot be.
            AstExpr::Str(_) => {
                return Err(Diagnostic::new(
                    "a string literal is only a narrowing argument here; there are no strings yet (M3)",
                    span,
                ));
            }
            AstExpr::Bool(v) => (Expr::Bool(*v), Type::Bool),
            AstExpr::Name(name) => {
                let text = self.ast.name_of(*name);
                match self.lookup(*name) {
                    Some(binding) => {
                        let (slot, ty) = (binding.slot, binding.ty.clone());
                        // Slice 1 has no borrowing, so every read of a `res`
                        // binding is a move. §5 adds the other kind.
                        self.trace.emit(Event::Use { slot, span });
                        (Expr::Load(slot), ty)
                    }
                    None if self.signatures.iter().any(|s| s.name == *name)
                        || Builtin::from_name(text).is_some() =>
                    {
                        return Err(Diagnostic::new(
                            format!(
                                "`{text}` is a function; M1 has no function values, so it can only be called"
                            ),
                            span,
                        ));
                    }
                    None => {
                        return Err(Diagnostic::new(format!("`{text}` is not bound here"), span));
                    }
                }
            }
            AstExpr::StructLit { name, fields } => {
                let text = self.ast.name_of(*name);
                let Some(def) = self.defs.iter().find(|d| d.name == *name) else {
                    return Err(Diagnostic::new(format!("`{text}` is not a struct"), span));
                };
                if is_capability(def.def) {
                    return Err(Diagnostic::new(
                        format!(
                            "`{text}` is a capability and has no literal form; authority comes from the `World` the runtime hands `main`, and from nowhere else"
                        ),
                        span,
                    ));
                }
                let DefKind::Struct(fields_decl) = &def.kind else {
                    return Err(Diagnostic::new(
                        format!("`{text}` is an enum, not a struct"),
                        span,
                    ));
                };
                // Copied out before checking any field value, because
                // checking borrows `self` and the table lives beside it.
                let (def_id, generic_count) = (def.def, def.generics.len());
                let fields_decl = fields_decl.clone();
                // A generic struct's arguments are inferred from the values
                // given for its fields, or left for the context to settle.
                let type_args = self.fresh_args(generic_count);
                let declared: Vec<(Symbol, Type)> =
                    fields_decl.iter().map(|(n, t)| (*n, t.substitute(&type_args, &[]))).collect();

                let mut values: Vec<Option<Expr>> = vec![None; declared.len()];
                let mut written_so_far: Option<usize> = None;
                for (field, value) in fields {
                    let field_text = self.ast.name_of(*field);
                    let Some(index) = declared.iter().position(|(n, _)| n == field) else {
                        return Err(Diagnostic::new(
                            format!("`{text}` has no field `{field_text}`"),
                            span,
                        ));
                    };
                    if values[index].is_some() {
                        return Err(Diagnostic::new(
                            format!("field `{field_text}` is given twice"),
                            span,
                        ));
                    }
                    // The fields run in *declaration* order, because that is
                    // the order a struct's leaves are laid out in and the
                    // order the backend evaluates them. Writing them in some
                    // other order would mean side effects happening in an
                    // order the text does not show -- so it is refused
                    // rather than silently reordered
                    // (`docs/defined-behaviour.md`). The order you read is
                    // the order it runs.
                    if let Some(previous) = written_so_far
                        && index < previous
                    {
                        return Err(Diagnostic::new(
                            format!(
                                "field `{field_text}` is written after `{}`, but `{text}` declares it before; a struct literal's fields run in declaration order, so writing them in another order would hide what runs first",
                                self.ast.name_of(declared[previous].0)
                            ),
                            self.ast.expr_span(*value),
                        ));
                    }
                    written_so_far = Some(index);
                    let value_span = self.ast.expr_span(*value);
                    let (lowered, found) = self.expr(*value)?;
                    self.expect_type(&declared[index].1, &found, value_span)?;
                    values[index] = Some(lowered);
                }

                // A struct value has every field or it is not one. There is no
                // default to fall back on and no zero to invent.
                if let Some(missing) = values.iter().position(Option::is_none) {
                    return Err(Diagnostic::new(
                        format!(
                            "missing field `{}` in `{text}`",
                            self.ast.name_of(declared[missing].0)
                        ),
                        span,
                    ));
                }

                (
                    Expr::Struct {
                        def: def_id,
                        fields: values.into_iter().map(|v| v.expect("checked above")).collect(),
                    },
                    Type::Named(def_id, type_args),
                )
            }
            AstExpr::Field { base, name } => {
                let base_span = self.ast.expr_span(*base);
                let (lowered, base_ty) = self.expr(*base)?;
                let mut resolved = self.unifier.resolve(&base_ty);
                // `r.x` where `r : &r Point` reads through the reference.
                // One level: a reference to a reference has to be written
                // through twice, because auto-dereferencing a chain is the
                // kind of convenience that makes a cost invisible.
                let through_reference = matches!(resolved, Type::Ref { .. });
                if let Type::Ref { inner, .. } = resolved {
                    resolved = *inner;
                }
                let Type::Named(def_id, type_args) = resolved else {
                    return Err(Diagnostic::new(
                        format!("`{}` has no fields", self.unifier.display(&resolved)),
                        base_span,
                    ));
                };
                let def = self
                    .defs
                    .iter()
                    .find(|d| d.def == def_id)
                    .expect("a named type is a declared type");
                let field_text = self.ast.name_of(*name);
                let DefKind::Struct(fields) = &def.kind else {
                    return Err(Diagnostic::new(
                        format!(
                            "`{}` is an enum; its payload is read by matching on it, not with `.`",
                            self.ast.name_of(def.name)
                        ),
                        base_span,
                    ));
                };
                let Some(index) = fields.iter().position(|(n, _)| n == name) else {
                    return Err(Diagnostic::new(
                        format!("`{}` has no field `{field_text}`", self.ast.name_of(def.name)),
                        span,
                    ));
                };
                let ty = fields[index].1.substitute(&type_args, &[]);
                if through_reference {
                    // Reading through a reference is what a reference is
                    // *for*, so no `Read` event: the referent is frozen for
                    // the whole region and nothing is being moved.
                    return Ok((
                        Expr::FieldRef {
                            base: Box::new(lowered),
                            def: def_id,
                            args: type_args,
                            index: index as u32,
                        },
                        ty,
                    ));
                }
                self.trace.emit(Event::Read {
                    ty: Type::Named(def_id, type_args.clone()),
                    span: base_span,
                });
                (
                    Expr::Field {
                        base: Box::new(lowered),
                        def: def_id,
                        args: type_args,
                        index: index as u32,
                    },
                    ty,
                )
            }
            AstExpr::Variant { enum_name, variant, args } => {
                let enum_text = self.ast.name_of(*enum_name);
                let variant_text = self.ast.name_of(*variant);
                let Some(def) = self.defs.iter().find(|d| d.name == *enum_name) else {
                    return Err(Diagnostic::new(format!("`{enum_text}` is not an enum"), span));
                };
                let DefKind::Enum(variants) = &def.kind else {
                    return Err(Diagnostic::new(
                        format!("`{enum_text}` is a struct, not an enum"),
                        span,
                    ));
                };
                let Some(index) = variants.iter().position(|(n, _)| n == variant) else {
                    return Err(Diagnostic::new(
                        format!("`{enum_text}` has no variant `{variant_text}`"),
                        span,
                    ));
                };
                let (def_id, generic_count) = (def.def, def.generics.len());
                let declared_payload = variants[index].1.clone();
                // Inferred from the payload values, or left for the context:
                // `Option::None` learns its `T` from where it is used.
                let type_args = self.fresh_args(generic_count);
                let payload_types: Vec<Type> =
                    declared_payload.iter().map(|t| t.substitute(&type_args, &[])).collect();

                if args.len() != payload_types.len() {
                    return Err(Diagnostic::new(
                        format!(
                            "`{enum_text}::{variant_text}` carries {} value{}, but {} {} given",
                            payload_types.len(),
                            if payload_types.len() == 1 { "" } else { "s" },
                            args.len(),
                            if args.len() == 1 { "was" } else { "were" }
                        ),
                        span,
                    ));
                }

                let mut payload = Vec::with_capacity(args.len());
                for (&arg, expected) in args.iter().zip(payload_types.iter()) {
                    let arg_span = self.ast.expr_span(arg);
                    let (value, found) = self.expr(arg)?;
                    self.expect_type(expected, &found, arg_span)?;
                    payload.push(value);
                }

                (
                    Expr::Enum {
                        def: def_id,
                        args: type_args.clone(),
                        variant: index as u32,
                        payload,
                    },
                    Type::Named(def_id, type_args),
                )
            }
            AstExpr::Unary { op, operand } => {
                let operand_span = self.ast.expr_span(*operand);
                let (inner, found) = self.expr(*operand)?;
                match op {
                    ast::UnOp::Neg => {
                        self.expect_type(&Type::Int, &found, operand_span)?;
                        (Expr::Neg(Box::new(inner)), Type::Int)
                    }
                    ast::UnOp::Not => {
                        self.expect_type(&Type::Bool, &found, operand_span)?;
                        (Expr::Not(Box::new(inner)), Type::Bool)
                    }
                }
            }
            AstExpr::Binary { op, lhs, rhs } => {
                let op = bin_op(*op);
                let lhs_span = self.ast.expr_span(*lhs);
                let rhs_span = self.ast.expr_span(*rhs);
                let (l, lt) = self.expr(*lhs)?;
                // `&&` and `||` do not evaluate their right operand when the
                // left already decides, so anything it consumes is consumed
                // conditionally — the same join as an `if` with no `else`.
                let short_circuit = op.is_short_circuit();
                if short_circuit {
                    self.trace.open();
                }
                let (r, rt) = self.expr(*rhs)?;
                if short_circuit {
                    let events = self.trace.close();
                    self.trace.emit(Event::Branch { arms: vec![events, Vec::new()], span });
                }

                // Both sides agree first, then the operator says what it
                // accepts. Reporting in that order blames the operand that
                // disagrees rather than the operator.
                self.expect_type(&lt, &rt, rhs_span)?;
                let operand = self.unifier.resolve(&lt);
                match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                        self.expect_type(&Type::Int, &operand, lhs_span)?;
                    }
                    BinOp::And | BinOp::Or => {
                        self.expect_type(&Type::Bool, &operand, lhs_span)?;
                    }
                    BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        self.expect_type(&Type::Int, &operand, lhs_span)?;
                    }
                    // `==` and `!=` compare two values of the same *scalar*
                    // type. Structs would need a field-wise comparison, which
                    // is a decision about what equality means rather than a
                    // missing instruction, so M1 refuses instead of guessing.
                    BinOp::Eq | BinOp::Ne => {
                        if !matches!(operand, Type::Int | Type::Bool) {
                            return Err(Diagnostic::new(
                                format!(
                                    "`{}` cannot be compared with `==` (M1 compares `int` and `bool`)",
                                    self.unifier.display(&operand)
                                ),
                                lhs_span,
                            ));
                        }
                    }
                }
                let result = op.result(&operand);
                (Expr::Bin { op, lhs: Box::new(l), rhs: Box::new(r) }, result)
            }
            AstExpr::Alloc { region, value } => return self.alloc(*region, *value, span),
            AstExpr::AllocSlice { region, count, fill } => {
                return self.alloc_slice(*region, *count, *fill, span);
            }
            AstExpr::Index { base, index } => return self.index(*base, *index, span),
            AstExpr::Call { callee, args } => {
                let text = self.ast.name_of(*callee);
                if self.lookup(*callee).is_some() {
                    return Err(Diagnostic::new(
                        format!("`{text}` is a local binding, not a function"),
                        span,
                    ));
                }

                // A generic callee is instantiated with fresh variables, which
                // the argument types then solve. The signature is all a caller
                // is ever checked against, generic or not.
                let mut instantiate: Option<(usize, Vec<Type>)> = None;
                let mut region_args: Vec<Region> = Vec::new();
                let mut foreign: Option<u32> = None;
                // §7.2: a body's row is a union over its calls. Taken here,
                // at the one place a call is resolved, so there is no second
                // walk that could disagree about what the body does.
                let performed = match Builtin::from_name(text) {
                    Some(builtin) => builtin.effects(),
                    None => self
                        .signatures
                        .iter()
                        .find(|sig| sig.name == *callee)
                        .map(|sig| sig.effects.clone())
                        .or_else(|| {
                            self.externs.iter().find(|e| e.name == text).map(|e| e.effects.clone())
                        })
                        .unwrap_or_default(),
                };
                self.performed.union(&performed);
                // §7.4: narrowing is checked here because both its argument
                // and its result depend on the literal that was written.
                if Builtin::from_name(text) == Some(Builtin::Narrow) {
                    return self.narrow(args, span);
                }
                if Builtin::from_name(text) == Some(Builtin::Release) {
                    return self.release(args, span);
                }
                if Builtin::from_name(text) == Some(Builtin::Len) {
                    return self.len(args, span);
                }
                let (params, ret) = if let Some(builtin) = Builtin::from_name(text) {
                    // A builtin's region parameters are instantiated exactly
                    // like a written function's (§5.1): one fresh region per
                    // parameter, solved by the arguments.
                    let fresh: Vec<Region> =
                        (0..builtin.regions()).map(|_| self.unifier.fresh_region()).collect();
                    let prelude = self.prelude();
                    let (params, ret) = builtin.signature(&prelude);
                    (
                        params.iter().map(|t| t.substitute(&[], &fresh)).collect::<Vec<_>>(),
                        ret.substitute(&[], &fresh),
                    )
                } else if let Some(index) = self.externs.iter().position(|e| e.name == text) {
                    // A foreign call is checked against its declaration and
                    // nothing else, exactly like a written function's (§8.4).
                    // Its region parameters are instantiated here too, since
                    // the capability it takes is borrowed.
                    let ext = &self.externs[index];
                    let (params, ret) = (ext.params.clone(), ext.ret.clone());
                    let count = params
                        .iter()
                        .filter_map(|t| match t {
                            Type::Ref { region: Region::Param(i), .. } => Some(*i + 1),
                            _ => None,
                        })
                        .max()
                        .unwrap_or(0) as usize;
                    let fresh: Vec<Region> =
                        (0..count).map(|_| self.unifier.fresh_region()).collect();
                    foreign = Some(index as u32);
                    (
                        params.iter().map(|t| t.substitute(&[], &fresh)).collect::<Vec<_>>(),
                        ret.substitute(&[], &fresh),
                    )
                } else {
                    let index = self.signatures.iter().position(|s| s.name == *callee).ok_or_else(
                        || {
                            Diagnostic::new(
                                format!("`{text}` is not a function in this unit"),
                                span,
                            )
                        },
                    )?;
                    let fresh = self.fresh_args(self.signatures[index].generics.len());
                    // §5.1: each region parameter gets a variable the
                    // argument types then solve. One name, one assignment.
                    let fresh_regions: Vec<Region> = (0..self.signatures[index].regions.len())
                        .map(|_| self.unifier.fresh_region())
                        .collect();
                    let signature = &self.signatures[index];
                    let params: Vec<Type> = signature
                        .params
                        .iter()
                        .map(|t| t.substitute(&fresh, &fresh_regions))
                        .collect();
                    let ret = signature.ret.substitute(&fresh, &fresh_regions);
                    instantiate = Some((index, fresh));
                    region_args = fresh_regions;
                    (params, ret)
                };

                if args.len() != params.len() {
                    return Err(Diagnostic::new(
                        format!(
                            "`{text}` takes {} argument{}, but {} {} given",
                            params.len(),
                            if params.len() == 1 { "" } else { "s" },
                            args.len(),
                            if args.len() == 1 { "was" } else { "were" }
                        ),
                        span,
                    ));
                }

                let mut lowered = Vec::with_capacity(args.len());
                for (&arg, expected) in args.iter().zip(params.iter()) {
                    let arg_span = self.ast.expr_span(arg);
                    let (value, found) = self.expr(arg)?;
                    self.expect_type(expected, &found, arg_span)?;
                    lowered.push(value);
                }
                // Every `where a <= b` the callee declared must hold between
                // the regions it was instantiated at. Same lexical lookup the
                // body used, so a caller discharges the obligation with the
                // same walk up the same stack (§5.2).
                if let Some((index, _)) = instantiate {
                    let obligations = self.signatures[index].outlives.clone();
                    let names = self.signatures[index].regions.clone();
                    for (inner, outer) in obligations {
                        let got_inner = self.unifier.resolve_region(region_args[inner as usize]);
                        let got_outer = self.unifier.resolve_region(region_args[outer as usize]);
                        if !self.outlives(got_outer, got_inner) {
                            return Err(Diagnostic::new(
                                format!(
                                    "`{text}` requires `{} <= {}`, but here `{}` does not outlive `{}`",
                                    self.ast.name_of(names[inner as usize]),
                                    self.ast.name_of(names[outer as usize]),
                                    self.unifier.display_region(got_outer),
                                    self.unifier.display_region(got_inner)
                                ),
                                span,
                            ));
                        }
                    }
                }

                let callee_ref = match instantiate {
                    None if foreign.is_some() => {
                        Callee::Extern(foreign.expect("checked by the guard"))
                    }
                    None => Callee::Builtin(
                        Builtin::from_name(text).expect("only a builtin skips instantiation"),
                    ),
                    Some((index, fresh)) => {
                        // Every type argument must be settled by the arguments.
                        // Letting the surrounding context settle one would mean
                        // deciding which copy to emit after the call was already
                        // lowered.
                        let mut settled = Vec::with_capacity(fresh.len());
                        for (position, var) in fresh.iter().enumerate() {
                            let resolved = self.unifier.resolve(var);
                            if resolved.is_var() {
                                let parameter =
                                    self.ast.name_of(self.signatures[index].generics[position]);
                                return Err(Diagnostic::new(
                                    format!(
                                        "cannot tell what `{parameter}` is in this call to `{text}`; it is not determined by the arguments"
                                    ),
                                    span,
                                ));
                            }
                            settled.push(resolved);
                        }
                        Callee::Fn(self.mono.request(index, settled))
                    }
                };
                (Expr::Call { callee: callee_ref, args: lowered }, ret)
            }
        })
    }
}

fn bin_op(op: ast::BinOp) -> BinOp {
    match op {
        ast::BinOp::Add => BinOp::Add,
        ast::BinOp::Sub => BinOp::Sub,
        ast::BinOp::Mul => BinOp::Mul,
        ast::BinOp::Div => BinOp::Div,
        ast::BinOp::Rem => BinOp::Rem,
        ast::BinOp::Eq => BinOp::Eq,
        ast::BinOp::Ne => BinOp::Ne,
        ast::BinOp::Lt => BinOp::Lt,
        ast::BinOp::Le => BinOp::Le,
        ast::BinOp::Gt => BinOp::Gt,
        ast::BinOp::Ge => BinOp::Ge,
        ast::BinOp::And => BinOp::And,
        ast::BinOp::Or => BinOp::Or,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_sys_syntax::parse;

    pub(super) fn lower_src(src: &str) -> Result<Program, Diagnostic> {
        lower(&parse(src).expect("should parse"))
    }

    fn error(src: &str) -> String {
        lower_src(src).expect_err("should be refused").message
    }

    fn main_fn(src: &str) -> Func {
        lower_src(src).expect("should check").funcs.pop().expect("a function")
    }

    // ---- resolution ----------------------------------------------------

    #[test]
    fn parameters_take_the_first_slots() {
        let f = main_fn("fn f(a: int, b: int) -> [] int { let c = a + b; return c; }");
        assert_eq!(f.n_params, 2);
        assert_eq!(f.n_slots(), 3);
        assert_eq!(
            f.body[0],
            Stmt::Store {
                place: Place::Slot(Slot(2)),
                value: Expr::Bin {
                    op: BinOp::Add,
                    lhs: Box::new(Expr::Load(Slot(0))),
                    rhs: Box::new(Expr::Load(Slot(1))),
                }
            }
        );
    }

    #[test]
    fn functions_are_visible_before_they_are_defined() {
        let p =
            lower_src("fn f() -> [] int { return g(); } fn g() -> [] int { return 1; }").unwrap();
        assert_eq!(p.funcs.len(), 2);
        assert_eq!(p.find("g"), Some(FuncId(1)));
    }

    #[test]
    fn an_inner_block_may_shadow() {
        let f = main_fn(
            "fn main[&i](io: &!i Io) -> [io] int { \
             let x = 1; if true { let x = 2; putchar(io, x); } return x; }",
        );
        // The capability, `x`, and the shadowing `x`.
        assert_eq!(f.n_slots(), 3);
    }

    #[test]
    fn rebinding_in_the_same_block_is_refused() {
        assert!(
            error("fn f() -> [] int { let x = 1; let x = 2; return x; }").contains("already bound")
        );
    }

    #[test]
    fn an_initialiser_cannot_see_its_own_binding() {
        assert!(error("fn f() -> [] int { let x = x; return x; }").contains("not bound"));
    }

    #[test]
    fn assigning_to_a_let_is_refused() {
        assert!(error("fn f() -> [] int { let x = 1; x = 2; return x; }").contains("immutable"));
    }

    #[test]
    fn assigning_to_a_parameter_is_refused() {
        assert!(error("fn f(a: int) -> [] int { a = 1; return a; }").contains("immutable"));
    }

    #[test]
    fn assigning_to_a_var_is_allowed() {
        assert!(lower_src("fn f() -> [] int { var x = 1; x = 2; return x; }").is_ok());
    }

    #[test]
    fn unknown_names_are_refused() {
        assert!(error("fn f() -> [] int { return nope; }").contains("not bound"));
        assert!(error("fn f() -> [] int { return nope(); }").contains("not a function"));
    }

    #[test]
    fn arity_is_checked_for_functions_and_builtins() {
        assert!(
            error("fn g(a: int) -> [] int { return a; } fn f() -> [] int { return g(); }")
                .contains("takes 1 argument")
        );
        assert!(
            error("fn f[&i](io: &!i Io) -> [] int { return putchar(io); }")
                .contains("takes 2 arguments")
        );
    }

    #[test]
    fn a_function_is_not_a_value() {
        assert!(
            error("fn g() -> [] int { return 1; } fn f() -> [] int { return g; }")
                .contains("no function values")
        );
    }

    #[test]
    fn a_local_is_not_callable() {
        assert!(error("fn f() -> [] int { let g = 1; return g(); }").contains("not a function"));
    }

    #[test]
    fn duplicate_definitions_are_refused() {
        assert!(
            error("fn f() -> [] int { return 1; } fn f() -> [] int { return 2; }")
                .contains("defined twice")
        );
        assert!(error("fn f(a: int, a: int) -> [] int { return a; }").contains("bound twice"));
    }

    #[test]
    fn builtins_cannot_be_redefined() {
        assert!(error("fn putchar(c: int) -> [] int { return c; }").contains("builtin"));
    }

    // ---- control flow --------------------------------------------------

    #[test]
    fn every_path_must_return() {
        assert!(error("fn f() -> [] int { let x = 1; }").contains("without returning"));
        assert!(error("fn f() -> [] int { if true { return 1; } }").contains("without returning"));
        assert!(lower_src("fn f() -> [] int { if true { return 1; } else { return 2; } }").is_ok());
        // Conservative on purpose: a loop never counts as a terminator.
        assert!(error("fn f() -> [] int { while true { } }").contains("without returning"));
    }

    #[test]
    fn code_after_a_return_is_refused() {
        assert!(error("fn f() -> [] int { return 1; return 2; }").contains("unreachable"));
    }

    // ---- types ---------------------------------------------------------

    #[test]
    fn slot_types_are_recorded_for_the_backend() {
        let f = main_fn("fn f(a: int, b: bool) -> [] int { let c = b; let d = a; return d; }");
        assert_eq!(f.slots, vec![Type::Int, Type::Bool, Type::Bool, Type::Int]);
        assert_eq!(f.ret, Type::Int);
    }

    #[test]
    fn a_let_takes_its_type_from_its_initialiser() {
        let f = main_fn("fn f() -> [] bool { let x = 1 < 2; return x; }");
        assert_eq!(f.slots, vec![Type::Bool]);
    }

    #[test]
    fn an_annotation_must_agree_with_the_initialiser() {
        assert!(
            error("fn f() -> [] int { let x: int = true; return x; }")
                .contains("expected `int`, found `bool`")
        );
        assert!(lower_src("fn f() -> [] bool { let x: bool = true; return x; }").is_ok());
    }

    #[test]
    fn a_condition_must_be_a_bool() {
        assert!(
            error("fn f() -> [] int { if 1 { return 0; } return 1; }")
                .contains("expected `bool`, found `int`")
        );
        assert!(
            error("fn f() -> [] int { while 1 { } return 1; }")
                .contains("expected `bool`, found `int`")
        );
    }

    #[test]
    fn return_must_match_the_signature() {
        assert!(
            error("fn f() -> [] int { return true; }").contains("expected `int`, found `bool`")
        );
        assert!(error("fn f() -> [] bool { return 1; }").contains("expected `bool`, found `int`"));
    }

    #[test]
    fn an_argument_must_match_the_parameter() {
        assert!(
            error("fn f[&i](io: &!i Io) -> [io] int { return putchar(io, true); }")
                .contains("expected `int`, found `bool`")
        );
    }

    #[test]
    fn an_assignment_must_match_the_binding() {
        assert!(
            error("fn f() -> [] int { var x = 1; x = true; return x; }")
                .contains("expected `int`, found `bool`")
        );
    }

    #[test]
    fn arithmetic_is_for_ints_and_logic_is_for_bools() {
        assert!(error("fn f() -> [] int { return true + true; }").contains("expected `int`"));
        assert!(error("fn f() -> [] bool { return 1 && 2; }").contains("expected `bool`"));
        assert!(error("fn f() -> [] int { return -true; }").contains("expected `int`"));
        assert!(error("fn f() -> [] bool { return !1; }").contains("expected `bool`"));
    }

    #[test]
    fn a_comparison_yields_a_bool() {
        let f = main_fn("fn f() -> [] bool { return 1 < 2; }");
        assert_eq!(f.ret, Type::Bool);
        // ...and ordering is for ints only.
        assert!(error("fn f() -> [] bool { return true < false; }").contains("expected `int`"));
    }

    #[test]
    fn equality_compares_two_values_of_the_same_type() {
        assert!(lower_src("fn f() -> [] bool { return 1 == 2; }").is_ok());
        assert!(lower_src("fn f() -> [] bool { return true == false; }").is_ok());
        assert!(error("fn f() -> [] bool { return 1 == true; }").contains("expected `int`"));
    }

    #[test]
    fn unknown_types_are_refused_where_they_are_written() {
        assert!(error("fn f() -> [] i32 { return 0; }").contains("unknown type `i32`"));
        assert!(error("fn f(a: i32) -> [] int { return 0; }").contains("unknown type `i32`"));
        assert!(
            error("fn f() -> [] int { let x: i32 = 1; return x; }").contains("unknown type `i32`")
        );
    }

    #[test]
    fn a_primitive_takes_no_type_arguments() {
        assert!(error("fn f() -> [] int[bool] { return 0; }").contains("takes no type arguments"));
    }

    // ---- structs -------------------------------------------------------

    #[test]
    fn a_struct_literal_is_positional_and_written_in_declaration_order() {
        // The IR holds the fields positionally, so the backend never has to
        // consult a field name.
        let f =
            main_fn("struct P { x: int, y: bool } fn f() -> [] P { return P { x: 7, y: true }; }");
        let Stmt::Return(Expr::Struct { fields, .. }) = &f.body[0] else { panic!("{:?}", f.body) };
        assert_eq!(fields[0], Expr::Int(7));
        assert_eq!(fields[1], Expr::Bool(true));

        // And writing them in another order is refused rather than silently
        // reordered: the fields run in declaration order, so any other
        // written order would hide which one runs first
        // (`docs/defined-behaviour.md`). This used to reorder quietly.
        let message = lower_src(
            "struct P { x: int, y: bool } fn f() -> [] P { return P { y: true, x: 7 }; } \
             fn main() -> [] int { return 0; }",
        )
        .expect_err("should be refused")
        .message;
        assert!(message.contains("declaration order"), "{message}");
    }

    #[test]
    fn a_field_access_becomes_an_index() {
        let f = main_fn("struct P { x: int, y: int } fn f(p: P) -> [] int { return p.y; }");
        let Stmt::Return(Expr::Field { index, .. }) = &f.body[0] else { panic!() };
        assert_eq!(*index, 1);
    }

    #[test]
    fn a_struct_literal_must_give_every_field_exactly_once() {
        let decl = "struct P { x: int, y: int } ";
        assert!(
            error(&format!("{decl}fn f() -> [] P {{ return P {{ x: 1 }}; }}"))
                .contains("missing field `y`")
        );
        assert!(
            error(&format!("{decl}fn f() -> [] P {{ return P {{ x: 1, y: 2, z: 3 }}; }}"))
                .contains("has no field `z`")
        );
        assert!(
            error(&format!("{decl}fn f() -> [] P {{ return P {{ x: 1, x: 2, y: 3 }}; }}"))
                .contains("given twice")
        );
        assert!(
            error(&format!("{decl}fn f() -> [] P {{ return P {{ x: true, y: 2 }}; }}"))
                .contains("expected `int`, found `bool`")
        );
    }

    #[test]
    fn fields_are_checked_against_the_declaration() {
        assert!(
            error("struct P { x: int } fn f(p: P) -> [] int { return p.z; }")
                .contains("`P` has no field `z`")
        );
        assert!(
            error("fn f() -> [] int { let x = 1; return x.y; }").contains("`int` has no fields")
        );
    }

    #[test]
    fn structs_may_nest_and_be_passed_by_value() {
        let p = lower_src(
            "struct P { x: int } struct L { a: P, b: P }              fn mid(l: L) -> [] int { return (l.a.x + l.b.x) / 2; }              fn f() -> [] int { return mid(L { a: P { x: 1 }, b: P { x: 3 } }); }",
        );
        assert!(p.is_ok(), "{:?}", p.err());
    }

    #[test]
    fn a_struct_that_contains_itself_is_refused() {
        // No references in M1, so this has no finite size. Both the direct and
        // the mutual case, because the check is reachability rather than a
        // look at one field.
        assert!(
            error("struct N { next: N } fn f() -> [] int { return 0; }")
                .contains("contains itself")
        );
        assert!(
            error("struct A { b: B } struct B { a: A } fn f() -> [] int { return 0; }")
                .contains("contains itself")
        );
        // ...but two fields of the same struct type are perfectly finite.
        assert!(
            lower_src("struct P { x: int } struct L { a: P, b: P } fn f() -> [] int { return 0; }")
                .is_ok()
        );
    }

    #[test]
    fn struct_declarations_are_checked_for_duplicates() {
        assert!(
            error("struct P { x: int } struct P { y: int } fn f() -> [] int { return 0; }")
                .contains("declared twice")
        );
        assert!(
            error("struct P { x: int, x: bool } fn f() -> [] int { return 0; }")
                .contains("field `x` is declared twice")
        );
        assert!(
            error("struct int { x: int } fn f() -> [] int { return 0; }").contains("built-in type")
        );
    }

    #[test]
    fn a_struct_may_mention_one_declared_later() {
        assert!(
            lower_src("struct A { b: B } struct B { x: int } fn f() -> [] int { return 0; }")
                .is_ok()
        );
    }

    #[test]
    fn structs_are_not_compared_with_equality() {
        assert!(
            error("struct P { x: int } fn f(a: P, b: P) -> [] bool { return a == b; }")
                .contains("cannot be compared")
        );
    }

    // ---- enums and match -------------------------------------------------

    const SHAPE: &str = "enum Shape { Empty, Circle(int), Rect(int, int) } ";

    #[test]
    fn a_variant_becomes_an_index_and_a_payload() {
        let f = main_fn(&format!("{SHAPE}fn f() -> [] Shape {{ return Shape::Rect(2, 3); }}"));
        let Stmt::Return(Expr::Enum { variant, payload, .. }) = &f.body[0] else { panic!() };
        assert_eq!(*variant, 2);
        assert_eq!(payload, &vec![Expr::Int(2), Expr::Int(3)]);
    }

    #[test]
    fn a_match_must_cover_every_variant() {
        let message = error(&format!(
            "{SHAPE}fn f(s: Shape) -> [] int {{ match s {{ Shape::Empty => {{ return 0; }} }} }}"
        ));
        assert!(message.contains("does not cover"), "{message}");
        assert!(message.contains("`Shape::Circle`"), "{message}");
        assert!(message.contains("`Shape::Rect`"), "{message}");
    }

    #[test]
    fn a_wildcard_covers_the_rest() {
        assert!(
            lower_src(&format!(
                "{SHAPE}fn f(s: Shape) -> [] int {{ match s {{ Shape::Empty => {{ return 0; }} _ => {{ return 1; }} }} }}"
            ))
            .is_ok()
        );
    }

    #[test]
    fn a_wildcard_that_covers_nothing_is_refused() {
        // Every variant is already matched, so the `_` can never run. Saying
        // so is worth more than silently allowing dead code.
        let message = error(&format!(
            "{SHAPE}fn f(s: Shape) -> [] int {{ match s {{              Shape::Empty => {{ return 0; }} Shape::Circle(r) => {{ return r; }}              Shape::Rect(w, h) => {{ return w * h; }} _ => {{ return 9; }} }} }}"
        ));
        assert!(message.contains("already matched"), "{message}");
    }

    #[test]
    fn arms_after_a_wildcard_are_refused() {
        let message = error(&format!(
            "{SHAPE}fn f(s: Shape) -> [] int {{ match s {{ _ => {{ return 0; }} Shape::Empty => {{ return 1; }} }} }}"
        ));
        assert!(message.contains("unreachable"), "{message}");
    }

    #[test]
    fn a_variant_may_not_be_matched_twice() {
        let message = error(&format!(
            "{SHAPE}fn f(s: Shape) -> [] int {{ match s {{              Shape::Empty => {{ return 0; }} Shape::Empty => {{ return 1; }} _ => {{ return 2; }} }} }}"
        ));
        assert!(message.contains("matched twice"), "{message}");
    }

    #[test]
    fn payload_arity_is_checked_when_building_and_when_matching() {
        assert!(
            error(&format!("{SHAPE}fn f() -> [] Shape {{ return Shape::Rect(1); }}"))
                .contains("carries 2 values, but 1 was given")
        );
        let message = error(&format!(
            "{SHAPE}fn f(s: Shape) -> [] int {{ match s {{ Shape::Rect(w) => {{ return w; }} _ => {{ return 0; }} }} }}"
        ));
        assert!(message.contains("the pattern binds 1"), "{message}");
    }

    #[test]
    fn a_binding_takes_the_payload_type() {
        // `r` is an `int`, so returning it from an `int` function is fine and
        // returning it from a `bool` one is not.
        assert!(
            lower_src(&format!(
                "{SHAPE}fn f(s: Shape) -> [] int {{ match s {{ Shape::Circle(r) => {{ return r; }} _ => {{ return 0; }} }} }}"
            ))
            .is_ok()
        );
        assert!(
            error(&format!(
                "{SHAPE}fn f(s: Shape) -> [] bool {{ match s {{ Shape::Circle(r) => {{ return r; }} _ => {{ return true; }} }} }}"
            ))
            .contains("expected `bool`, found `int`")
        );
    }

    #[test]
    fn two_arms_may_bind_the_same_name_at_different_types() {
        assert!(
            lower_src(
                "enum E { A(int), B(bool) }                  fn f(e: E) -> [] int { match e { E::A(v) => { return v; } E::B(v) => { if v { return 1; } return 0; } } }"
            )
            .is_ok()
        );
    }

    #[test]
    fn an_exhaustive_match_where_every_arm_returns_is_a_terminator() {
        // No trailing `return` needed: the match itself covers every path.
        assert!(
            lower_src(&format!(
                "{SHAPE}fn f(s: Shape) -> [] int {{ match s {{                  Shape::Empty => {{ return 0; }} Shape::Circle(r) => {{ return r; }}                  Shape::Rect(w, h) => {{ return w * h; }} }} }}"
            ))
            .is_ok()
        );
    }

    #[test]
    fn only_enums_are_matched() {
        assert!(
            error("fn f() -> [] int { match 1 { _ => { return 0; } } }")
                .contains("`int` cannot be matched")
        );
        assert!(
            error("struct P { x: int } fn f(p: P) -> [] int { match p { _ => { return 0; } } }")
                .contains("is a struct, not an enum")
        );
    }

    #[test]
    fn an_enum_that_contains_itself_is_refused() {
        assert!(
            error("enum List { Nil, Cons(int, List) } fn f() -> [] int { return 0; }")
                .contains("contains itself")
        );
    }

    #[test]
    fn an_enum_needs_at_least_one_variant() {
        assert!(error("enum Void { } fn f() -> [] int { return 0; }").contains("has no variants"));
    }

    #[test]
    fn structs_and_enums_are_not_interchangeable() {
        assert!(
            error("struct P { x: int } fn f() -> [] int { let p = P::x(1); return 0; }")
                .contains("is a struct, not an enum")
        );
        assert!(
            error("enum E { A } fn f() -> [] int { let e = E { x: 1 }; return 0; }")
                .contains("is an enum, not a struct")
        );
        assert!(
            error("enum E { A(int) } fn f(e: E) -> [] int { return e.x; }")
                .contains("read by matching on it")
        );
    }

    // ---- generics --------------------------------------------------------

    fn names(src: &str) -> Vec<String> {
        let mut names: Vec<String> =
            lower_src(src).expect("should check").funcs.into_iter().map(|f| f.name).collect();
        names.sort();
        names
    }

    #[test]
    fn a_generic_function_is_copied_once_per_instantiation() {
        let names = names(
            "fn id[T](x: T) -> [] T { return x; }              fn main() -> [] int { if id(true) { return id(1); } return id(2); }",
        );
        // One copy per type, not per call: `id(1)` and `id(2)` share theirs.
        assert_eq!(names, ["id$bool", "id$int", "main"]);
    }

    #[test]
    fn a_generic_function_nobody_calls_is_emitted_nowhere() {
        let names =
            names("fn unused[T](x: T) -> [] T { return x; } fn main() -> [] int { return 0; }");
        assert_eq!(names, ["main"]);
    }

    #[test]
    fn a_generic_body_is_checked_even_when_it_is_never_called() {
        // The point of checking rigidly: an error in a generic function does
        // not wait for someone to instantiate it.
        assert!(
            error(
                "fn unused[T](x: T) -> [] int { return true; } fn main() -> [] int { return 0; }"
            )
            .contains("expected `int`, found `bool`")
        );
    }

    #[test]
    fn a_type_parameter_is_rigid_inside_the_body() {
        // `T` is not `int`, however every instantiation so far might be.
        let message =
            error("fn bad[T](x: T) -> [] T { return x + 1; } fn main() -> [] int { return 0; }");
        assert!(message.contains("expected `T`"), "{message}");
    }

    #[test]
    fn type_arguments_are_inferred_from_the_arguments() {
        assert!(
            lower_src("fn id[T](x: T) -> [] T { return x; } fn main() -> [] int { return id(1); }")
                .is_ok()
        );
        assert!(
            error(
                "fn same[T](a: T, b: T) -> [] T { return a; }                  fn main() -> [] int { return same(1, true); }"
            )
            .contains("expected `int`, found `bool`")
        );
    }

    #[test]
    fn a_type_argument_the_arguments_do_not_settle_is_refused() {
        let message = error(
            "enum Opt[T] { None, Some(T) }              fn make[T]() -> [] Opt[T] { return Opt::None; }              fn main() -> [] int { let x = make(); return 0; }",
        );
        assert!(message.contains("cannot tell what `T` is"), "{message}");
    }

    #[test]
    fn a_generic_struct_substitutes_its_arguments_into_field_types() {
        assert!(
            lower_src(
                "struct Pair[A, B] { first: A, second: B }                  fn main() -> [] int { let p = Pair { first: 1, second: true };                  if p.second { return p.first; } return 0; }"
            )
            .is_ok()
        );
        assert!(
            error(
                "struct Pair[A, B] { first: A, second: B }                  fn main() -> [] int { let p = Pair { first: 1, second: true }; return p.second; }"
            )
            .contains("expected `int`, found `bool`")
        );
    }

    #[test]
    fn a_generic_enum_substitutes_its_arguments_into_payloads() {
        assert!(
            lower_src(
                "enum Opt[T] { None, Some(T) }                  fn f(o: Opt[int]) -> [] int { match o { Opt::None => { return 0; } Opt::Some(v) => { return v; } } }"
            )
            .is_ok()
        );
        // The binding is an `int` here, so returning it as a `bool` is wrong.
        assert!(
            error(
                "enum Opt[T] { None, Some(T) }                  fn f(o: Opt[int]) -> [] bool { match o { Opt::None => { return true; } Opt::Some(v) => { return v; } } }"
            )
            .contains("expected `bool`, found `int`")
        );
    }

    #[test]
    fn a_nullary_variant_takes_its_type_from_the_context() {
        // Nothing in `Opt::None` says what `T` is; the annotation does.
        assert!(
            lower_src(
                "enum Opt[T] { None, Some(T) }                  fn main() -> [] int { let x: Opt[int] = Opt::None; return 0; }"
            )
            .is_ok()
        );
        assert!(
            error(
                "enum Opt[T] { None, Some(T) } fn main() -> [] int { let x = Opt::None; return 0; }"
            )
            .contains("cannot tell what type")
        );
    }

    #[test]
    fn type_argument_counts_are_checked() {
        assert!(
            error(
                "struct Pair[A, B] { first: A, second: B }                  fn f(p: Pair[int]) -> [] int { return p.first; }"
            )
            .contains("takes 2 type arguments, but 1 was given")
        );
        assert!(
            error("fn f(x: int[bool]) -> [] int { return 0; }").contains("takes no type arguments")
        );
        assert!(
            error("fn f[T](x: T[int]) -> [] int { return 0; }")
                .contains("type parameter `T` takes no type arguments")
        );
    }

    #[test]
    fn type_parameter_names_are_checked() {
        assert!(error("fn f[T, T](x: T) -> [] T { return x; }").contains("declared twice"));
        assert!(error("fn f[int](x: int) -> [] int { return x; }").contains("built-in type"));
        assert!(
            error("struct S[A, A] { x: A } fn main() -> [] int { return 0; }")
                .contains("declared twice")
        );
    }

    #[test]
    fn a_signature_is_checked_against_never_a_body() {
        // `g`'s body returns a bool, and its signature says int. The call in
        // `f` is checked against the signature, so the error is reported in
        // `g` -- a caller never learns anything from a callee's body.
        let message = error("fn g() -> [] int { return true; } fn f() -> [] int { return g(); }");
        assert!(message.contains("expected `int`, found `bool`"), "{message}");
    }
}

/// Effect rows: `docs/linearity-and-effects.md` §7.
#[cfg(test)]
mod effect_tests {
    use super::tests::lower_src;
    use super::{Effects, Label, Program};

    fn refused(src: &str) -> String {
        lower_src(src).expect_err("this should be refused").message
    }

    fn accepted(src: &str) -> Program {
        lower_src(src).expect("this should be accepted")
    }

    #[test]
    fn a_row_is_a_canonically_ordered_set() {
        // §7.1: no duplicates, and an order that does not depend on which
        // label a file happened to mention first.
        let a = Effects::plain(["io", "fs", "io"]);
        let b = Effects::plain(["fs", "io"]);
        assert_eq!(a, b);
        assert_eq!(a.to_string(), "[fs, io]");
        // §7.4: a label's argument is part of its identity, and the order is
        // still the text's rather than the order of mention.
        let narrowed = Effects::new([
            Label { name: "ffi".to_owned(), argument: Some("libm".to_owned()) },
            Label { name: "ffi".to_owned(), argument: Some("libc".to_owned()) },
            Label { name: "ffi".to_owned(), argument: Some("libc".to_owned()) },
        ]);
        assert_eq!(narrowed.to_string(), "[ffi(\"libc\"), ffi(\"libm\")]");
    }

    #[test]
    fn union_and_subset_are_the_only_operations_needed() {
        let mut row = Effects::pure();
        assert!(row.is_pure());
        row.union(&Effects::plain(["io"]));
        row.union(&Effects::plain(["io", "fs"]));
        assert_eq!(row.to_string(), "[fs, io]");
        assert_eq!(row.missing_from(&Effects::plain(["fs", "io"])), None);
        assert_eq!(
            row.missing_from(&Effects::plain(["io"])).map(Label::to_string),
            Some("fs".to_owned())
        );
    }

    #[test]
    fn a_call_widens_the_callers_row() {
        let message = refused(
            "fn quiet[&i](io: &!i Io) -> [] int { putchar(io, 65); return 0; } \
             fn main() -> [] int { return 0; }",
        );
        assert!(message.contains("performs `io`"), "{message}");
    }

    #[test]
    fn an_over_wide_row_is_an_error_not_a_warning() {
        let message = refused("fn f() -> [io] int { return 1; } fn main() -> [] int { return 0; }");
        assert!(message.contains("never performs it"), "{message}");
    }

    #[test]
    fn a_row_is_transitive() {
        let message = refused(
            "fn shout[&i](io: &!i Io) -> [io] int { return putchar(io, 33); } \
             fn caller[&i](io: &!i Io) -> [] int { return shout(io); }",
        );
        assert!(message.contains("`caller` performs `io`"), "{message}");
        accepted(
            "fn shout[&i](io: &!i Io) -> [io] int { return putchar(io, 33); } \
             fn caller[&i](io: &!i Io) -> [io] int { return shout(io); }",
        );
    }

    #[test]
    fn an_ungrounded_label_can_never_be_exact() {
        // No registry of legal labels, and none needed: nothing performs
        // `telepathy`, so no exact row can contain it.
        let message =
            refused("fn f() -> [telepathy] int { return 1; } fn main() -> [] int { return 0; }");
        assert!(message.contains("declares `telepathy`"), "{message}");
    }

    #[test]
    fn a_duplicate_label_is_the_same_row() {
        accepted(
            "fn f[&i](io: &!i Io) -> [io, io] int { return putchar(io, 33); } \
             fn caller[&i](io: &!i Io) -> [io] int { return f(io); }",
        );
    }

    #[test]
    fn a_pure_helper_inside_an_effectful_body_adds_nothing() {
        accepted(
            "fn double(n: int) -> [] int { return n * 2; } \
             fn main[&i](io: &!i Io) -> [io] int { return putchar(io, double(20)); }",
        );
    }

    #[test]
    fn an_effect_in_a_branch_still_counts() {
        // The row is a union over the calls the body contains, not over the
        // ones a particular run reaches. Anything else would need to know
        // which branch is taken.
        let message = refused(
            "fn f[&i](io: &!i Io, c: bool) -> [] int { if c { putchar(io, 65); } return 0; } \
             fn main() -> [] int { return 0; }",
        );
        assert!(message.contains("performs `io`"), "{message}");
    }

    #[test]
    fn a_generic_functions_row_is_not_per_instantiation() {
        // Rows live on signatures, so monomorphisation does not touch them:
        // one row however many copies the backend emits.
        let program = accepted(
            "fn id[T](x: T) -> [] T { return x; } \
             fn main[&i](io: &!i Io) -> [io] int { return putchar(io, id(65)) - 65; }",
        );
        let copies: Vec<&str> =
            program.funcs.iter().map(|f| f.name.as_str()).filter(|n| n.starts_with("id")).collect();
        assert_eq!(copies.len(), 1, "{copies:?}");
        let id = program.func(program.find(copies[0]).expect("id"));
        assert!(id.effects.is_pure(), "{}", id.effects);
    }

    #[test]
    fn the_row_reaches_the_lowered_function() {
        let program =
            accepted("fn main[&i](io: &!i Io) -> [io] int { return putchar(io, 65) - 65; }");
        let main = program.func(program.find("main").expect("main"));
        assert_eq!(main.effects.to_string(), "[io]");
    }
}

/// Capabilities: `docs/linearity-and-effects.md` §8.
#[cfg(test)]
mod capability_tests {
    use super::Program;
    use super::tests::lower_src;

    /// Enough of a program to have authority in it.
    const MAIN: &str = " fn main(world: World) -> [] int { \
        let Split { io, ffi } = split(world); release(ffi); release(io); return 0; }";

    fn refused(src: &str) -> String {
        lower_src(src).expect_err("this should be refused").message
    }

    fn accepted(src: &str) -> Program {
        lower_src(src).expect("this should be accepted")
    }

    #[test]
    fn a_capability_has_no_literal_form() {
        // The rule §8.2 rests on: if this were writable, a function given
        // nothing could still print.
        for name in ["Io", "World", "Split"] {
            let src = format!("fn f() -> [] int {{ let c = {name} {{ }}; return 0; }}{MAIN}");
            let message = refused(&src);
            assert!(message.contains("has no literal form"), "{name}: {message}");
        }
    }

    #[test]
    fn a_capability_type_cannot_be_redeclared() {
        let message = refused(&format!("res struct Io {{ fd: int }}{MAIN}"));
        assert!(message.contains("cannot be redeclared"), "{message}");
    }

    #[test]
    fn authority_comes_from_the_world_and_must_be_released() {
        accepted(&format!("fn f() -> [] int {{ return 0; }}{MAIN}"));

        let leaked = refused(
            "fn main(world: World) -> [] int { let Split { io, ffi } = split(world); release(ffi); return 0; }",
        );
        assert!(leaked.contains("still live"), "{leaked}");

        let world = refused("fn main(world: World) -> [] int { return 0; }");
        assert!(world.contains("`world` is still live"), "{world}");
    }

    #[test]
    fn a_released_capability_cannot_be_used_again() {
        let message = refused(
            "fn greet[&i](io: &!i Io) -> [io] int { return putchar(io, 65); } \
             fn main(world: World) -> [] int { let Split { io, ffi } = split(world); release(ffi); \
             release(io); borrow mut io as &!i in { greet(i); } return 0; }",
        );
        assert!(message.contains("nothing left to borrow"), "{message}");
    }

    #[test]
    fn a_capability_is_destroyed_by_release_not_by_destructuring() {
        let message = refused(
            "fn main(world: World) -> [] int { let Split { io, ffi } = split(world); release(ffi); \
             let Io { } = io; return 0; }",
        );
        assert!(message.contains("destroyed by `release`"), "{message}");
    }

    #[test]
    fn owning_a_capability_discharges_its_label() {
        // §8.2: a row lists what a function *borrows*. `main` prints and its
        // row is still `[]`, because it owns the authority outright -- and
        // that is visible in the parameter list rather than in the row.
        let program = accepted(
            "fn greet[&i](io: &!i Io) -> [io] int { return putchar(io, 65); } \
             fn main(world: World) -> [] int { let Split { io, ffi } = split(world); release(ffi); \
             borrow mut io as &!i in { greet(i); } release(io); return 0; }",
        );
        let main = program.func(program.find("main").expect("main"));
        assert!(main.effects.is_pure(), "{}", main.effects);
    }

    #[test]
    fn borrowing_a_capability_does_not_discharge_it() {
        // The other half of the same rule, and the one that keeps every row
        // in the program from being empty.
        let message = refused("fn quiet[&i](io: &!i Io) -> [] int { putchar(io, 65); return 0; }");
        assert!(message.contains("performs `io`"), "{message}");
    }

    #[test]
    fn a_capability_costs_nothing_at_runtime() {
        // §8.1: capabilities erase except where they carry data, and these
        // carry none. `World` and `Io` are zero-sized, so threading one adds
        // no machine parameter at all.
        let program = accepted(&format!("fn f() -> [] int {{ return 0; }}{MAIN}"));
        let main = program.func(program.find("main").expect("main"));
        assert_eq!(main.n_params, 1, "`main` takes the World");
        assert!(
            super::leaf_free(&main.slots[0]),
            "a `World` should occupy no machine value, got {:?}",
            main.slots[0]
        );
    }
}

/// Slices: M3's first half, built on §5's references.
#[cfg(test)]
mod slice_tests {
    use super::Program;
    use super::tests::lower_src;

    fn refused(src: &str) -> String {
        lower_src(src).expect_err("this should be refused").message
    }

    fn accepted(src: &str) -> Program {
        lower_src(src).expect("this should be accepted")
    }

    /// `main` with `body` inside, for the cases about a region rather than
    /// about a signature.
    fn in_main(body: &str) -> String {
        format!("fn main() -> [] int {{ {body} return 0; }}")
    }

    #[test]
    fn a_slice_is_a_reference_and_carries_its_region() {
        accepted(&in_main("region a { let xs = alloc_slice[a](3, 0); xs[0] = 1; let n = xs[0]; }"));

        // And it cannot outlive the arena it points into -- the same
        // occurs-check that stops a `borrow`'s reference escaping, run by
        // the same code. A slice adds no escape rule of its own.
        let message = refused(
            "fn escape[&q](fallback: &q [int]) -> [] &q [int] \
             { region a { return alloc_slice[a](1, 0); } } \
             fn main() -> [] int { return 0; }",
        );
        assert!(message.contains("may not outlive its region"), "{message}");
    }

    #[test]
    fn an_unsized_referent_is_not_a_value() {
        // `[T]`'s length is a runtime value rather than part of its type,
        // so there is nothing to lay out. Only a reference may point at one.
        for written in [
            "fn f(xs: [int]) -> [] int { return 0; }",
            "struct S { xs: [int] }",
            "fn f[&r](xs: &r [[int]]) -> [] int { return 0; }",
        ] {
            let message = refused(&format!("{written} fn main() -> [] int {{ return 0; }}"));
            assert!(message.contains("no size of its own"), "{written}: {message}");
        }
    }

    #[test]
    fn a_slice_holds_val_data_only() {
        // §6.1, sharpened: the fill is copied into every element, and a
        // linear value cannot be copied at all.
        let message = refused(
            "res struct File { fd: int } \
             fn open(n: int) -> [] File { return File { fd: n }; } \
             fn main() -> [] int { region a { let s = alloc_slice[a](2, open(1)); } return 0; }",
        );
        assert!(message.contains("arena holds `val` data only"), "{message}");
    }

    #[test]
    fn a_shared_slice_may_not_be_written_through() {
        let message = refused(
            "fn clobber[&r](xs: &r [int]) -> [] int { xs[0] = 1; return 0; } \
             fn main() -> [] int { return 0; }",
        );
        assert!(message.contains("shared slice"), "{message}");

        // The unique one may, and the coercion the other way still holds:
        // a function wanting `&r [int]` accepts the `&!a [int]` `alloc_slice`
        // hands back.
        accepted(
            "fn total[&r](xs: &r [int]) -> [] int { return len(xs); } \
             fn main() -> [] int { region a { let xs = alloc_slice[a](2, 0); \
             xs[0] = 1; let n = total(xs); } return 0; }",
        );
    }

    #[test]
    fn only_a_slice_can_be_indexed_or_measured() {
        let indexed = refused(&in_main("let x = 3; let y = x[0];"));
        assert!(indexed.contains("is not a slice"), "{indexed}");

        let measured = refused(&in_main("let x = 3; let n = len(x);"));
        assert!(measured.contains("is not a slice"), "{measured}");
    }

    #[test]
    fn an_index_is_an_int_and_so_is_a_length() {
        let bad_index =
            refused(&in_main("region a { let xs = alloc_slice[a](2, 0); let n = xs[true]; }"));
        assert!(bad_index.contains("expected `int`"), "{bad_index}");

        let bad_count = refused(&in_main("region a { let xs = alloc_slice[a](true, 0); }"));
        assert!(bad_count.contains("expected `int`"), "{bad_count}");
    }

    #[test]
    fn a_slices_element_type_comes_from_its_fill() {
        // No annotation anywhere: `alloc_slice[a](3, true)` is a slice of
        // `bool` because the fill is one, and indexing it yields `bool`.
        accepted(&in_main(
            "region a { let flags = alloc_slice[a](3, true); \
             if flags[0] { let n = 1; } }",
        ));

        // And the element type is invariant, like every other referent.
        let message = refused(
            "fn ints[&r](xs: &r [int]) -> [] int { return len(xs); } \
             fn main() -> [] int { region a { let flags = alloc_slice[a](2, true); \
             let n = ints(flags); } return 0; }",
        );
        assert!(message.contains("expected"), "{message}");
    }
}

/// Defined behaviour: `docs/defined-behaviour.md`.
#[cfg(test)]
mod defined_behaviour_tests {
    use super::tests::lower_src;
    use super::{Builtin, Callee, Expr, Program, Stmt};

    fn refused(src: &str) -> String {
        lower_src(src).expect_err("this should be refused").message
    }

    fn accepted(src: &str) -> Program {
        lower_src(src).expect("this should be accepted")
    }

    #[test]
    fn wrapping_arithmetic_is_spelled_out() {
        // §2.2: `+` means arithmetic, `wrapping_add` means the bits. The
        // asymmetry is what stops the second happening by accident.
        let program = accepted(
            "fn f(a: int, b: int) -> [] int { return wrapping_add(a, wrapping_mul(b, 2)); } \
             fn main() -> [] int { return f(1, 2) - 5; }",
        );
        let f = program.func(program.find("f").expect("f"));
        let Some(Stmt::Return(Expr::Call { callee, .. })) = f.body.first() else {
            panic!("a call, got {:?}", f.body.first())
        };
        assert_eq!(*callee, Callee::Builtin(Builtin::WrappingAdd));
    }

    #[test]
    fn wrapping_arithmetic_is_pure_and_needs_no_capability() {
        // It observes nothing outside the program, so it is not an effect
        // and an empty row still means pure.
        let program = accepted(
            "fn f(a: int) -> [] int { return wrapping_mul(a, 3); } \
             fn main() -> [] int { return f(0); }",
        );
        let f = program.func(program.find("f").expect("f"));
        assert!(f.effects.is_pure(), "{}", f.effects);
    }

    #[test]
    fn a_wrapping_builtin_cannot_be_redefined() {
        let message = refused(
            "fn wrapping_add(a: int, b: int) -> [] int { return a; } \
             fn main() -> [] int { return 0; }",
        );
        assert!(message.contains("is a builtin"), "{message}");
    }

    #[test]
    fn a_struct_literal_runs_in_the_order_it_is_written() {
        // §3: the order you read is the order it runs, kept true by
        // refusing the literal that would break it rather than by
        // reordering underneath the text.
        let message = refused(
            "struct P { x: int, y: int } \
             fn main() -> [] int { let p = P { y: 2, x: 1 }; return p.x; }",
        );
        assert!(message.contains("declaration order"), "{message}");

        // Written in declaration order, the same literal is fine.
        accepted(
            "struct P { x: int, y: int } \
             fn main() -> [] int { let p = P { x: 1, y: 2 }; return p.x - 1; }",
        );

        // A single field, or fields that skip none, are unaffected: the rule
        // is about relative order, not about naming every field in a row.
        accepted(
            "struct Q { a: int, b: int, c: int } \
             fn main() -> [] int { let q = Q { a: 1, b: 2, c: 3 }; return q.a - 1; }",
        );
    }
}

/// Arenas: `docs/linearity-and-effects.md` §6.
#[cfg(test)]
mod arena_tests {
    use super::tests::lower_src;
    use super::{Program, Stmt};

    const NODE: &str = "struct Node { value: int } ";

    fn refused(src: &str) -> String {
        lower_src(src).expect_err("this should be refused").message
    }

    fn accepted(src: &str) -> Program {
        lower_src(src).expect("this should be accepted")
    }

    /// `main` with `body` inside, for the cases about a region rather than
    /// about a signature.
    fn in_main(body: &str) -> String {
        format!("{NODE} fn main() -> [] int {{ {body} return 0; }}")
    }

    #[test]
    fn an_arena_is_a_region_and_alloc_hands_back_a_unique_reference() {
        let program = accepted(&in_main(
            "region a { let n = alloc[a](Node { value: 1 }); let v = n.value; }",
        ));
        let main = program.func(program.find("main").expect("main"));
        assert!(
            matches!(main.body.first(), Some(Stmt::Region { arena: 0, .. })),
            "a `region` lowers to an arena, got {:?}",
            main.body.first()
        );
    }

    #[test]
    fn nothing_mentioning_the_arena_escapes_it() {
        // §6, and the same occurs-check §5 runs -- which is the claim: an
        // arena's lifetime and a borrow's lifetime are one mechanism.
        let message = refused(&format!(
            "{NODE} fn escape[&q](fallback: &q Node) -> [] &q Node \
             {{ region a {{ return alloc[a](Node {{ value: 1 }}); }} }} \
             fn main() -> [] int {{ return 0; }}"
        ));
        assert!(message.contains("may not outlive its region"), "{message}");
    }

    #[test]
    fn an_inner_arenas_reference_may_not_be_stored_in_an_outer_one() {
        // Nesting is §5.2's stack. The outer arena outlives the inner, so
        // references go inwards and never back out.
        let message = refused(&in_main(
            "region o { var held = alloc[o](Node { value: 1 }); \
             region i { held = alloc[i](Node { value: 2 }); } }",
        ));
        assert!(message.contains("does not outlive"), "{message}");

        // And the permitted direction is genuinely permitted.
        accepted(&in_main(
            "region o { let base = alloc[o](Node { value: 1 }); \
             region i { let n = base.value + alloc[i](Node { value: 2 }).value; } }",
        ));
    }

    #[test]
    fn an_arena_holds_val_data_only() {
        // §6.1: releasing an arena reclaims memory and runs nothing, so a
        // linear obligation put inside would be dropped rather than
        // discharged -- a leak with a static blessing.
        let message = refused(
            "res struct File { fd: int } \
             fn open(n: int) -> [] File { return File { fd: n }; } \
             fn main() -> [] int { let f = open(3); region a { let p = alloc[a](f); } return 0; }",
        );
        assert!(message.contains("arena holds `val` data only"), "{message}");
    }

    #[test]
    fn alloc_names_an_arena_that_is_open() {
        let unopened = refused(&in_main("let n = alloc[a](Node { value: 1 });"));
        assert!(unopened.contains("is not an arena open here"), "{unopened}");

        // A `borrow` block's region is a region, but it is not an arena:
        // there is no chunk behind it to allocate in.
        let borrowed = refused(&format!(
            "{NODE} fn main() -> [] int {{ let b = Node {{ value: 1 }}; \
             borrow b as &r in {{ let n = alloc[r](Node {{ value: 2 }}); }} return 0; }}"
        ));
        assert!(borrowed.contains("is not an arena open here"), "{borrowed}");

        // Nor is a region *parameter*: a caller's region is not this
        // function's to allocate in.
        let parameter = refused(&format!(
            "{NODE} fn f[&q](n: &q Node) -> [] int {{ let p = alloc[q](Node {{ value: 1 }}); return 0; }} \
             fn main() -> [] int {{ return 0; }}"
        ));
        assert!(parameter.contains("is not an arena open here"), "{parameter}");
    }

    #[test]
    fn an_inner_arena_shadows_an_outer_one_of_the_same_name() {
        // Two arenas, two regions: `Region::Block` carries identity rather
        // than depth, so the inner `a` is not the outer `a`.
        let program =
            accepted(&in_main("region a { region a { let n = alloc[a](Node { value: 1 }); } }"));
        let main = program.func(program.find("main").expect("main"));
        let Some(Stmt::Region { arena: 0, body }) = main.body.first() else {
            panic!("the outer arena, got {:?}", main.body.first());
        };
        assert!(
            matches!(body.first(), Some(Stmt::Region { arena: 1, .. })),
            "the inner arena is a second one, got {:?}",
            body.first()
        );
    }

    #[test]
    fn a_unique_reference_is_accepted_where_a_shared_one_is_wanted() {
        // §6's one new coercion, and what makes arena data reachable from a
        // helper written against `&r`: `&!r T` is `&r T` plus permission to
        // write, so handing one over read-only gives away nothing.
        accepted(&format!(
            "{NODE} fn value_of[&r](n: &r Node) -> [] int {{ return n.value; }} \
             fn main() -> [] int {{ region a {{ let v = value_of(alloc[a](Node {{ value: 1 }})); }} return 0; }}"
        ));

        // The other direction stays refused: a shared reference promises the
        // referent will not change, and nothing may write through it.
        let message = refused(&format!(
            "{NODE} fn bump[&r](n: &!r Node) -> [] int {{ n.value = 1; return 0; }} \
             fn main() -> [] int {{ let b = Node {{ value: 1 }}; \
             borrow b as &r in {{ let x = bump(r); }} return 0; }}"
        ));
        assert!(message.contains("unique reference"), "{message}");
    }

    #[test]
    fn an_arena_reference_is_val_and_costs_one_pointer() {
        // A reference is `val` whatever it points at (§5 rule 3), so a
        // binding holding one owes nothing at scope end. Two of them from
        // the same arena is not a double anything.
        accepted(&in_main(
            "region a { let x = alloc[a](Node { value: 1 }); let y = x; let z = x.value; }",
        ));
    }
}

/// Narrowing and foreign calls: `docs/linearity-and-effects.md` §7.4 and §8.4.
#[cfg(test)]
mod foreign_tests {
    use super::tests::lower_src;
    use super::{Callee, Expr, Program, Stmt};

    /// libc's `long labs(long)`, which is the smallest foreign function that
    /// takes an argument, returns a result and cannot be mistaken for a
    /// builtin.
    const LABS: &str = "extern fn labs[&f](ffi: &f Ffi(\"libc\"), n: int) -> [ffi(\"libc\")] int; ";

    fn refused(src: &str) -> String {
        lower_src(src).expect_err("this should be refused").message
    }

    fn accepted(src: &str) -> Program {
        lower_src(src).expect("this should be accepted")
    }

    /// A `main` that takes the authority it is given and gives it back, for
    /// the cases whose subject is a declaration rather than a body.
    const MAIN: &str = " fn main(world: World) -> [] int { \
        let Split { io, ffi } = split(world); release(ffi); release(io); return 0; }";

    /// A `main` that narrows to libc, runs `body` inside the borrow, and
    /// gives everything back.
    fn with_libc(body: &str) -> String {
        format!(
            "{LABS} fn main(world: World) -> [] int {{ \
             let Split {{ io, ffi }} = split(world); release(io); \
             let libc = narrow(ffi, \"libc\"); var n = 0; \
             borrow libc as &f in {{ {body} }} \
             release(libc); return n; }}"
        )
    }

    #[test]
    fn a_foreign_call_needs_the_capability_that_names_its_library() {
        // §8.4, and the reason the declaration is where this is checked: it
        // is the only place a foreign signature is written.
        let message = refused(
            "extern fn labs(n: int) -> [ffi(\"libc\")] int; \
             fn main(world: World) -> [] int { let Split { io, ffi } = split(world); \
             release(ffi); release(io); return labs(0); }",
        );
        assert!(message.contains("holds no capability that authorises it"), "{message}");
    }

    #[test]
    fn a_foreign_row_is_exact_in_both_directions() {
        let quiet =
            refused(&format!("extern fn labs[&f](ffi: &f Ffi(\"libc\"), n: int) -> [] int;{MAIN}"));
        assert!(quiet.contains("does not declare"), "{quiet}");

        let wrong_library = refused(&format!(
            "extern fn labs[&f](ffi: &f Ffi(\"libc\"), n: int) -> [ffi(\"libm\")] int;{MAIN}"
        ));
        assert!(
            wrong_library.contains("holds no capability that authorises it"),
            "{wrong_library}"
        );
    }

    #[test]
    fn a_foreign_declaration_names_a_library() {
        // The unnarrowed root names none, so a declaration borrowing one
        // would be a foreign call with no library behind it.
        let message = refused(&format!(
            "extern fn labs[&f](ffi: &f Ffi(\"\"), n: int) -> [ffi(\"\")] int;{MAIN}"
        ));
        assert!(message.contains("names no library"), "{message}");
    }

    #[test]
    fn only_what_c_can_name_crosses_the_boundary() {
        let aggregate = refused(&format!(
            "struct P {{ x: int }} \
             extern fn f[&c](ffi: &c Ffi(\"libc\"), p: P) -> [ffi(\"libc\")] int;{MAIN}"
        ));
        assert!(aggregate.contains("no agreed layout"), "{aggregate}");

        let reference = refused(&format!(
            "struct P {{ x: int }} \
             extern fn f[&c, &r](ffi: &c Ffi(\"libc\"), p: &r P) -> [ffi(\"libc\")] int;{MAIN}"
        ));
        assert!(reference.contains("borrowed capability"), "{reference}");
    }

    #[test]
    fn a_foreign_name_is_not_also_a_written_function() {
        // Two answers to one call is one answer too many.
        let message = refused(&format!("{LABS} fn labs(n: int) -> [] int {{ return n; }}{MAIN}"));
        assert!(message.contains("already declared foreign"), "{message}");
    }

    #[test]
    fn narrowing_goes_one_way() {
        // §7.4: prefix extension, and `libc` is a prefix of `libcrypto`, so
        // the capability over `libcrypto` is the narrower of the two.
        let message = refused(
            "fn main(world: World) -> [] int { let Split { io, ffi } = split(world); \
             release(io); let crypto = narrow(ffi, \"libcrypto\"); \
             let wider = narrow(crypto, \"libc\"); release(wider); return 0; }",
        );
        assert!(message.contains("never widened"), "{message}");
    }

    #[test]
    fn narrowing_to_the_same_thing_is_refused() {
        let message = refused(
            "fn main(world: World) -> [] int { let Split { io, ffi } = split(world); \
             release(io); let a = narrow(ffi, \"libc\"); let b = narrow(a, \"libc\"); \
             release(b); return 0; }",
        );
        assert!(message.contains("grants nothing new"), "{message}");
    }

    #[test]
    fn narrowing_consumes_the_wider_capability() {
        // The point of the whole section: after narrowing there is no way
        // back to what was narrowed, because it was spent.
        let message = refused(
            "fn main(world: World) -> [] int { let Split { io, ffi } = split(world); \
             release(io); let libc = narrow(ffi, \"libc\"); release(libc); \
             let libm = narrow(ffi, \"libm\"); release(libm); return 0; }",
        );
        assert!(message.contains("has already been consumed"), "{message}");
    }

    #[test]
    fn a_borrowed_capability_cannot_be_narrowed() {
        let message = refused(
            "fn main(world: World) -> [] int { let Split { io, ffi } = split(world); \
             release(io); borrow ffi as &f in { let libc = narrow(f, \"libc\"); release(libc); } \
             release(ffi); return 0; }",
        );
        assert!(message.contains("narrowing consumes"), "{message}");
    }

    #[test]
    fn owning_the_world_discharges_every_library() {
        // §8.2's discharge rule, with a label that carries a value. `main`
        // owns the `World`, which can be split and narrowed to anything, so
        // the authority for `ffi("libc")` is already in its parameter list.
        let program = accepted(&with_libc("n = labs(f, 0 - 7);"));
        let main = program.func(program.find("main").expect("main"));
        assert!(main.effects.is_pure(), "{}", main.effects);
    }

    #[test]
    fn a_borrowed_narrowed_capability_declares_the_label_it_names() {
        let program = accepted(&format!(
            "{LABS} \
             fn size[&f](ffi: &f Ffi(\"libc\"), n: int) -> [ffi(\"libc\")] int \
             {{ return labs(ffi, n); }} \
             fn main(world: World) -> [] int {{ let Split {{ io, ffi }} = split(world); \
             release(io); let libc = narrow(ffi, \"libc\"); var n = 0; \
             borrow libc as &f in {{ n = size(f, 0 - 7); }} release(libc); return n - 7; }}"
        ));
        let size = program.func(program.find("size").expect("size"));
        assert_eq!(size.effects.to_string(), "[ffi(\"libc\")]");

        // And it is not interchangeable with the label for another library.
        let message = refused(&format!(
            "{LABS} \
             fn size[&f](ffi: &f Ffi(\"libc\"), n: int) -> [ffi(\"libm\")] int \
             {{ return labs(ffi, n); }}{MAIN}"
        ));
        assert!(message.contains("does not declare"), "{message}");
    }

    #[test]
    fn the_capability_travels_as_far_as_the_check_and_no_further() {
        // The IR keeps the capability as an argument, because a capability's
        // *journey* is what was checked and the argument still has to be
        // evaluated. Dropping it is the backend's job (§8.1), and
        // `tests/accept/narrowed_capability.ls` is what proves it happens.
        let program = accepted(&with_libc("n = labs(f, 0 - 7);"));
        let main = program.func(program.find("main").expect("main"));
        let mut foreign_calls = 0;
        for stmt in main.body.iter() {
            walk(stmt, &mut foreign_calls);
        }
        assert_eq!(foreign_calls, 1, "the foreign call should survive lowering");
    }

    /// Count `Callee::Extern` calls, checking each one's shape as it goes.
    fn walk(stmt: &Stmt, found: &mut u32) {
        let mut visit = |expr: &Expr| {
            if let Expr::Call { callee: Callee::Extern(_), args } = expr {
                assert_eq!(args.len(), 2, "the capability is still an argument here");
                *found += 1;
            }
        };
        match stmt {
            Stmt::Store { value, .. } | Stmt::Eval(value) | Stmt::Return(value) => visit(value),
            Stmt::Borrow { body, .. } => {
                for inner in body {
                    walk(inner, found);
                }
            }
            _ => {}
        }
    }
}

/// Modes and linearity: `docs/linearity-and-effects.md` §3 and §4.
#[cfg(test)]
mod linearity_tests {
    use super::tests::lower_src;
    use super::{Diagnostic, Program};

    /// A `res` type, a way to make one, and a way to spend one -- the three
    /// things every case below needs.
    const PRELUDE: &str = "\
        res struct File { fd: int } \
        fn open(n: int) -> [] File { return File { fd: n }; } \
        fn close(f: File) -> [] int { let File { fd } = f; return fd; } ";

    fn check(body: &str) -> Result<Program, Diagnostic> {
        lower_src(&format!("{PRELUDE}{body}"))
    }

    fn refused(body: &str) -> String {
        check(body).expect_err("this should be refused").message
    }

    fn accepted(body: &str) {
        check(body).expect("this should be accepted");
    }

    #[test]
    fn a_res_value_consumed_once_is_accepted() {
        accepted("fn main() -> [] int { return close(open(1)); }");
    }

    #[test]
    fn a_res_value_used_twice_is_refused() {
        let message = refused(
            "struct Pair { a: File, b: File } \
             fn main() -> [] int { let f = open(1); let p = Pair { a: f, b: f }; return 0; }",
        );
        assert!(message.contains("already been consumed"), "{message}");
    }

    #[test]
    fn a_res_value_used_after_a_move_is_refused() {
        let message =
            refused("fn main() -> [] int { let f = open(1); let a = close(f); return close(f); }");
        assert!(message.contains("already been consumed"), "{message}");
    }

    #[test]
    fn a_res_value_live_at_a_return_is_refused() {
        let message = refused("fn main() -> [] int { let f = open(1); return 0; }");
        assert!(message.contains("consumed on every path"), "{message}");
    }

    #[test]
    fn a_res_value_live_at_the_end_of_a_block_is_refused() {
        let message = refused("fn main() -> [] int { if true { let f = open(1); } return 0; }");
        assert!(message.contains("still live at the end of this block"), "{message}");
    }

    #[test]
    fn branches_must_agree_about_what_is_live() {
        let message = refused(
            "fn main() -> [] int { let f = open(1); if true { let a = close(f); } return 0; }",
        );
        assert!(message.contains("branches disagree about `f`"), "{message}");
    }

    #[test]
    fn branches_that_agree_are_accepted() {
        accepted(
            "fn main() -> [] int { let f = open(1); \
             if true { let a = close(f); } else { let b = close(f); } return 0; }",
        );
    }

    #[test]
    fn an_arm_that_returns_does_not_have_to_agree() {
        // A `return` is not at the merge point, so it takes no part in the
        // join. Without that, §4.1's own accepting example would be refused.
        accepted(
            "fn main() -> [] int { let f = open(1); \
             if true { return close(f); } return close(f); }",
        );
    }

    #[test]
    fn an_arm_may_create_and_spend_a_value_of_its_own() {
        // The `then` arm declares and consumes `f`; the empty `else` never
        // sees it. That is not a disagreement -- a binding declared inside an
        // arm dies with the arm, and its own block already checked it.
        accepted(
            "fn main() -> [] int { if true { let f = open(1); let a = close(f); } return 0; }",
        );
    }

    #[test]
    fn a_loop_may_not_consume_an_outer_binding() {
        let message = refused(
            "fn main() -> [] int { let f = open(1); var i = 0; \
             while i < 2 { let a = close(f); i = i + 1; } return 0; }",
        );
        assert!(message.contains("consumed inside this loop"), "{message}");
    }

    #[test]
    fn a_loop_that_consumes_what_it_creates_is_accepted() {
        accepted(
            "fn main() -> [] int { var i = 0; \
             while i < 2 { let f = open(i); let a = close(f); i = i + 1; } return 0; }",
        );
    }

    #[test]
    fn a_conditionally_evaluated_operand_is_a_branch() {
        // `&&` does not evaluate its right operand when the left decides, so
        // a consumption there happens on one path only.
        let message = refused(
            "fn spend(f: File) -> [] bool { let a = close(f); return true; } \
             fn main() -> [] int { let f = open(1); \
             let b = false && spend(f); return 0; }",
        );
        assert!(message.contains("branches disagree"), "{message}");
    }

    #[test]
    fn a_res_value_cannot_be_discarded() {
        let message = refused("fn main() -> [] int { open(1); return 0; }");
        assert!(message.contains("cannot be discarded"), "{message}");
    }

    #[test]
    fn a_val_value_may_be_discarded() {
        accepted("fn main() -> [] int { close(open(1)); return 0; }");
    }

    #[test]
    fn a_field_cannot_be_read_out_of_a_res_value() {
        let message =
            refused("fn main() -> [] int { let f = open(1); let n = f.fd; return close(f); }");
        assert!(message.contains("a field cannot be read out of it"), "{message}");
    }

    #[test]
    fn assigning_over_a_live_res_binding_is_refused() {
        let message =
            refused("fn main() -> [] int { var f = open(1); f = open(2); return close(f); }");
        assert!(message.contains("would discard the `res` value"), "{message}");
    }

    #[test]
    fn assigning_over_a_spent_res_binding_is_accepted() {
        accepted(
            "fn main() -> [] int { var f = open(1); let a = close(f); f = open(2); \
             return close(f); }",
        );
    }

    #[test]
    fn a_wildcard_arm_may_not_swallow_a_res_scrutinee() {
        let message = refused(
            "enum Slot { Empty, Full(File) } \
             fn size(s: Slot) -> [] int { match s { Slot::Empty => { return 0; } \
             _ => { return 1; } } } fn main() -> [] int { return 0; }",
        );
        assert!(message.contains("the value matched here is `res`"), "{message}");
    }

    #[test]
    fn an_ignored_res_payload_is_refused() {
        let message = refused(
            "enum Slot { Empty, Full(File) } \
             fn size(s: Slot) -> [] int { match s { Slot::Empty => { return 0; } \
             Slot::Full(_) => { return 1; } } } fn main() -> [] int { return 0; }",
        );
        assert!(message.contains("this payload is `res`"), "{message}");
    }

    #[test]
    fn an_ignored_val_payload_is_accepted() {
        accepted(
            "enum Slot { Empty, Full(int) } \
             fn size(s: Slot) -> [] int { match s { Slot::Empty => { return 0; } \
             Slot::Full(_) => { return 1; } } } fn main() -> [] int { return 0; }",
        );
    }

    #[test]
    fn mode_is_inferred_from_members() {
        let message = refused(
            "struct Holder { f: File } \
             fn main() -> [] int { let h = Holder { f: open(1) }; return 0; }",
        );
        assert!(message.contains("consumed on every path"), "{message}");
    }

    #[test]
    fn a_val_declaration_may_not_hold_a_res_member() {
        let message = refused("val struct Wrapper { f: File } fn main() -> [] int { return 0; }");
        assert!(message.contains("declared `val`, but it holds"), "{message}");
    }

    #[test]
    fn a_val_declaration_of_val_members_is_accepted() {
        accepted("val struct Point { x: int, y: int } fn main() -> [] int { return 0; }");
    }

    #[test]
    fn a_generic_type_takes_its_mode_from_its_arguments() {
        // `Held[int]` is `val` and may be dropped; `Held[File]` is `res`.
        accepted(
            "struct Held[T] { value: T } fn main() -> [] int { let h = Held { value: 1 }; return 0; }",
        );
        let message = refused(
            "struct Held[T] { value: T } \
             fn main() -> [] int { let h = Held { value: open(1) }; return 0; }",
        );
        assert!(message.contains("consumed on every path"), "{message}");
    }

    #[test]
    fn a_generic_function_is_checked_at_each_instantiation() {
        // The body is accepted where it is written, because a type parameter
        // is `val` (§3). The copy at `File` is where it fails, and the
        // message says which copy.
        let message = refused(
            "fn sink[T](x: T) -> [] int { return 0; } fn main() -> [] int { return sink(open(1)); }",
        );
        assert!(message.contains("instantiated at `File`"), "{message}");
        accepted(
            "fn sink[T](x: T) -> [] int { return 0; } fn main() -> [] int { return sink(1); }",
        );
    }

    #[test]
    fn destructuring_consumes_the_whole_and_produces_the_parts() {
        accepted(
            "struct Pair { a: File, b: File } \
             fn main() -> [] int { let p = Pair { a: open(1), b: open(2) }; \
             let Pair { a, b } = p; return close(a) + close(b); }",
        );
    }

    #[test]
    fn a_destructured_part_carries_its_own_obligation() {
        let message = refused(
            "struct Pair { a: File, b: File } \
             fn main() -> [] int { let p = Pair { a: open(1), b: open(2) }; \
             let Pair { a, b } = p; return close(a); }",
        );
        assert!(message.contains("consumed on every path"), "{message}");
    }

    #[test]
    fn a_partial_destructuring_is_refused() {
        let message = refused(
            "struct Pair { a: File, b: File } \
             fn main() -> [] int { let p = Pair { a: open(1), b: open(2) }; \
             let Pair { a } = p; return close(a); }",
        );
        assert!(message.contains("takes the whole value apart"), "{message}");
    }

    #[test]
    fn destructuring_names_the_declared_fields() {
        let message =
            refused("fn main() -> [] int { let File { handle } = open(1); return handle; }");
        assert!(message.contains("has no field `handle`"), "{message}");
        let message = refused("fn main() -> [] int { let Missing { x } = open(1); return x; }");
        assert!(message.contains("is not a struct"), "{message}");
    }

    #[test]
    fn destructuring_evaluates_its_value_once() {
        // Two fields, one call: the value goes into an unnamed slot and the
        // parts come out of it.
        let program = check(
            "struct Pair { a: int, b: int } \
             fn make() -> [] Pair { return Pair { a: 1, b: 2 }; } \
             fn main() -> [] int { let Pair { a, b } = make(); return a + b; }",
        )
        .expect("accepted");
        let main = program.func(program.find("main").expect("main"));
        let calls = format!("{:?}", main.body).matches("Call").count();
        assert_eq!(calls, 1, "{:?}", main.body);
    }

    #[test]
    fn an_enum_may_be_declared_res() {
        let message = refused(
            "res enum Handle { Closed, Open(int) } \
             fn main() -> [] int { let h = Handle::Closed; return 0; }",
        );
        assert!(message.contains("consumed on every path"), "{message}");
    }

    // ---- borrowing (`docs/linearity-and-effects.md` §5) -----------------

    #[test]
    fn a_borrow_block_that_returns_is_a_terminator() {
        // It runs once and unconditionally, so a function whose only `return`
        // is inside one has still returned. The backend agrees by asking the
        // same `terminates`.
        // A `val` referent, so nothing is owed when the block returns; a
        // `res` one would still have to be consumed on the way out, which is
        // a different rule doing its job.
        accepted(
            "struct C { n: int } \
             fn main() -> [] int { let c = C { n: 1 }; borrow c as &r in { return r.n - 1; } }",
        );
    }

    #[test]
    fn a_shared_borrow_reads_without_consuming() {
        accepted(
            "fn size[&p](h: &p File) -> [] int { return h.fd; } \
             fn main() -> [] int { let f = open(1); \
             borrow f as &r in { let n = size(r); } return close(f); }",
        );
    }

    #[test]
    fn a_reference_reads_a_field_its_referent_could_not() {
        // The owned value refuses `f.fd` (a part read without taking the
        // whole apart); the reference is exactly how that read is spelled.
        accepted(
            "fn main() -> [] int { let f = open(1); \
             borrow f as &r in { let n = r.fd; } return close(f); }",
        );
        let message =
            refused("fn main() -> [] int { let f = open(1); let n = f.fd; return close(f); }");
        assert!(message.contains("a field cannot be read out of it"), "{message}");
    }

    #[test]
    fn a_frozen_binding_cannot_be_moved() {
        let message = refused(
            "fn main() -> [] int { let f = open(1); \
             borrow f as &r in { let a = close(f); } return 0; }",
        );
        assert!(message.contains("frozen by an enclosing `borrow`"), "{message}");
    }

    #[test]
    fn a_frozen_binding_cannot_be_assigned_to() {
        let message = refused(
            "struct C { n: int } fn main() -> [] int { var c = C { n: 1 }; \
             borrow c as &r in { c = C { n: 2 }; } return 0; }",
        );
        assert!(message.contains("cannot be assigned to"), "{message}");
    }

    #[test]
    fn a_consumed_value_has_nothing_left_to_borrow() {
        let message = refused(
            "fn main() -> [] int { let f = open(1); let a = close(f); \
             borrow f as &r in { return a; } }",
        );
        assert!(message.contains("nothing left to borrow"), "{message}");
    }

    #[test]
    fn the_freeze_lifts_when_the_block_closes() {
        accepted(
            "fn main() -> [] int { let f = open(1); \
             borrow f as &r in { let n = r.fd; } return close(f); }",
        );
    }

    #[test]
    fn shared_borrows_nest() {
        // Freezing is not exclusive, and the inner block closing must not
        // thaw the outer one -- which is why the checker counts rather than
        // flags.
        accepted(
            "fn size[&p](h: &p File) -> [] int { return h.fd; } \
             fn main() -> [] int { let f = open(1); \
             borrow f as &a in { borrow f as &b in { let n = size(a) + size(b); } \
             let m = size(a); } return close(f); }",
        );
    }

    #[test]
    fn a_reference_may_not_outlive_its_region() {
        let message = refused(
            "fn escape[&q](f: File, fallback: &q File) -> [] &q File { \
             borrow f as &r in { return r; } return fallback; } \
             fn main() -> [] int { return 0; }",
        );
        assert!(message.contains("may not outlive its region"), "{message}");
    }

    #[test]
    fn a_reference_may_not_escape_through_inference_either() {
        // A binding declared outside the block whose type was still a hole
        // when the block opened. No `return` is involved, which is why rule 4
        // is checked over every binding and not only over what leaves.
        let message = refused(
            "enum Holder[T] { Empty, Full(T) } \
             fn main() -> [] int { let f = open(1); let hole = Holder::Empty; \
             borrow f as &r in { let used: Holder[&r File] = hole; } return close(f); }",
        );
        assert!(message.contains("would hold a reference into `r`"), "{message}");
    }

    #[test]
    fn a_region_must_be_in_scope_where_it_is_written() {
        let message = refused(
            "fn escape(f: File) -> [] &r File { return f; } fn main() -> [] int { return 0; }",
        );
        assert!(message.contains("is not a region in scope"), "{message}");
    }

    #[test]
    fn sibling_regions_do_not_outlive_each_other() {
        let message = refused(
            "fn same[&p](a: &p File, b: &p File) -> [] int { return 0; } \
             fn u(x: File, y: File) -> [] int { \
             borrow x as &a in { borrow y as &b in { let n = same(a, b); } } \
             return close(x) + close(y); } fn main() -> [] int { return 0; }",
        );
        assert!(message.contains("does not outlive"), "{message}");
    }

    #[test]
    fn an_outer_reference_is_usable_in_an_inner_block() {
        accepted(
            "fn size[&p](h: &p File) -> [] int { return h.fd; } \
             fn u(x: File, y: File) -> [] int { \
             borrow x as &a in { borrow y as &b in { let n = size(a) + size(b); } } \
             return close(x) + close(y); } fn main() -> [] int { return 0; }",
        );
    }

    #[test]
    fn a_declared_outlives_is_checked_at_the_call_site() {
        const OUTER_FIRST: &str = "fn copy_into[&dst, &src where src <= dst](d: &dst File, s: &src File) -> [] int { return 0; } \
             fn u(x: File, y: File) -> [] int { \
             borrow x as &outer in { borrow y as &inner in { let n = copy_into(PAIR); } } \
             return close(x) + close(y); } fn main() -> [] int { return 0; }";
        // `dst` is the outer block, which does outlive the inner `src`.
        accepted(&OUTER_FIRST.replace("PAIR", "outer, inner"));
        // And the other way round, which does not.
        let message = refused(&OUTER_FIRST.replace("PAIR", "inner, outer"));
        assert!(message.contains("requires `src <= dst`"), "{message}");
    }

    #[test]
    fn a_where_clause_names_the_declarations_own_regions() {
        let message = refused(
            "fn f[&a where b <= a](x: &a File) -> [] int { return 0; } fn main() -> [] int { return 0; }",
        );
        assert!(message.contains("is not a region parameter"), "{message}");
    }

    #[test]
    fn region_and_type_parameters_do_not_collide() {
        let message =
            refused("fn f[T, &T](x: T) -> [] int { return 0; } fn main() -> [] int { return 0; }");
        assert!(message.contains("both a type parameter and a region parameter"), "{message}");
        let message = refused(
            "fn f[&r, &r](x: &r File) -> [] int { return 0; } fn main() -> [] int { return 0; }",
        );
        assert!(message.contains("region parameter `r` is declared twice"), "{message}");
    }

    #[test]
    fn a_reference_is_val_and_may_be_copied_and_dropped() {
        // §5 rule 3. A reference to a `res` value is still `val`, which is
        // sound because the referent is frozen for the whole region.
        accepted(
            "fn main() -> [] int { let f = open(1); \
             borrow f as &r in { let a = r; let b = r; let n = a.fd + b.fd; } \
             return close(f); }",
        );
    }

    #[test]
    fn a_unique_borrow_locks_its_referent() {
        // §5 rule 2: nothing else may touch it at all, not even a read.
        let message = refused(
            "struct C { n: int } fn main() -> [] int { var c = C { n: 1 }; \
             borrow mut c as &!r in { let peek = c; } return 0; }",
        );
        assert!(message.contains("nothing else may read it"), "{message}");
    }

    #[test]
    fn there_is_at_most_one_unique_borrow() {
        let message = refused(
            "struct C { n: int } fn main() -> [] int { var c = C { n: 1 }; \
             borrow mut c as &!a in { borrow mut c as &!b in { return 0; } } }",
        );
        assert!(message.contains("already uniquely borrowed"), "{message}");
    }

    #[test]
    fn a_frozen_value_cannot_be_borrowed_uniquely() {
        let message = refused(
            "struct C { n: int } fn main() -> [] int { var c = C { n: 1 }; \
             borrow c as &s in { borrow mut c as &!u in { return 0; } } }",
        );
        assert!(message.contains("cannot be borrowed uniquely"), "{message}");
    }

    #[test]
    fn a_unique_borrow_releases_its_lock_at_the_end_of_the_block() {
        accepted(
            "struct C { n: int } fn main() -> [] int { var c = C { n: 1 }; \
             borrow mut c as &!a in { a.n = 2; } \
             borrow mut c as &!b in { b.n = 3; } return c.n - 3; }",
        );
    }

    #[test]
    fn copies_of_one_unique_reference_alias() {
        // `&!r` is `val`, so it copies. Both copies are copies of one
        // pointer, so the second write sees the first. Spilling the value in
        // and reading it back per copy would lose one of them, which is why
        // there is a single buffer per block rather than one per reference.
        accepted(
            "struct C { n: int } fn main() -> [] int { var c = C { n: 0 }; \
             borrow mut c as &!r in { let a = r; let b = r; a.n = 3; b.n = b.n + 4; } \
             return c.n - 7; }",
        );
    }

    #[test]
    fn a_shared_reference_may_not_be_written_through() {
        let message = refused(
            "struct C { n: int } fn main() -> [] int { var c = C { n: 1 }; \
             borrow c as &r in { r.n = 2; } return 0; }",
        );
        assert!(message.contains("shared reference"), "{message}");
    }

    #[test]
    fn a_field_of_an_owned_local_is_not_a_place() {
        let message = refused(
            "struct C { n: int } fn main() -> [] int { var c = C { n: 1 }; c.n = 2; return 0; }",
        );
        assert!(message.contains("assign the whole value instead"), "{message}");
    }

    #[test]
    fn a_write_through_a_reference_may_not_drop_a_res_field() {
        // Overwriting names no consumer for what was there, which is the
        // silent drop §4 refuses however it is spelled.
        let message = refused(
            "struct Holder { f: File } \
             fn set[&r](h: &!r Holder) -> [] int { h.f = open(2); return 0; } \
             fn main() -> [] int { return 0; }",
        );
        assert!(message.contains("cannot be discarded"), "{message}");
    }

    #[test]
    fn a_region_is_erased_and_does_not_copy_a_function() {
        // Regions have no runtime meaning, so a region-polymorphic function
        // is emitted once however many regions call it. A *type* parameter
        // still copies.
        let program = check(
            "fn size[&p](h: &p File) -> [] int { return h.fd; } \
             fn main() -> [] int { let f = open(1); \
             borrow f as &a in { let x = size(a); } \
             borrow f as &b in { let y = size(b); } return close(f); }",
        )
        .expect("accepted");
        let copies = program.funcs.iter().filter(|f| f.name.starts_with("size")).count();
        assert_eq!(copies, 1, "{:?}", program.funcs.iter().map(|f| &f.name).collect::<Vec<_>>());
    }

    #[test]
    fn the_linearity_check_says_where() {
        // Every rule here reports at a span the programmer wrote, not at the
        // enclosing function: the trace carries spans precisely so it can.
        let source = format!("{PRELUDE}fn main() -> [] int {{ let f = open(1); return 0; }}");
        let error = lower_src(&source).expect_err("refused");
        assert!(source[error.span.start as usize..error.span.end as usize].starts_with("return"));
    }
}
