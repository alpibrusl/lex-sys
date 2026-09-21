//! The AST, shaped for canonicalisation from the first line (#1, #3).
//!
//! Nothing hashes this yet — `docs/canonical-ast.md` is unwritten and per-unit
//! identity is M3 work. The shape is what has to be right now, because an AST
//! that is retrofitted for hashing is an AST whose hashes are already wrong:
//!
//! * **Nodes carry no spans.** Arenas hold nodes; parallel side tables hold
//!   spans. A hash walks the arenas and never sees a byte offset.
//! * **Nodes carry no formatting.** Whitespace, comments and redundant
//!   parentheses die in the lexer and parser. Two files that differ only in
//!   layout produce identical arenas.
//! * **Names are interned to dense indices,** so a node is plain old data of a
//!   fixed size. The interner is ordered by first appearance and is never
//!   iterated in hash order — no `HashMap` traversal reaches any output.
//! * **Every list is a `Vec` in source order.** No set, no map, no iteration
//!   order that depends on an address or a hash seed.

use crate::span::Span;
use std::collections::HashMap;

macro_rules! id_type {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub struct $name(pub u32);

        impl $name {
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

id_type!(/// Index of an expression in [`Ast::exprs`].
    ExprId);
id_type!(/// Index of a statement in [`Ast::stmts`].
    StmtId);
id_type!(/// Index of an item in [`Ast::items`].
    ItemId);
id_type!(/// Index of a written type in [`Ast::types`].
    TypeId);
id_type!(/// Index of an interned name in [`Interner`].
    Symbol);

/// Interned identifiers. Indices are assigned in order of first appearance, so
/// the table is a deterministic function of the token stream.
#[derive(Default, Debug)]
pub struct Interner {
    strings: Vec<String>,
    lookup: HashMap<String, Symbol>,
}

impl Interner {
    pub fn intern(&mut self, s: &str) -> Symbol {
        if let Some(&sym) = self.lookup.get(s) {
            return sym;
        }
        let sym = Symbol(self.strings.len() as u32);
        self.strings.push(s.to_owned());
        self.lookup.insert(s.to_owned(), sym);
        sym
    }

    pub fn resolve(&self, sym: Symbol) -> &str {
        &self.strings[sym.index()]
    }

    pub fn get(&self, s: &str) -> Option<Symbol> {
        self.lookup.get(s).copied()
    }

    pub fn len(&self) -> usize {
        self.strings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }
}

/// A type as it was *written*, not as it resolves.
///
/// The parser does not know which types exist — `int` and `Option[int]` parse
/// the same way, and deciding that `i32` names nothing is the checker's job,
/// which is where the program's meaning lives. Arguments are a `Vec` so
/// generics need no second syntax when they arrive.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TypeExpr {
    /// `int`, `Pair[int, bool]`, `io.Buffer` — a name, optionally
    /// qualified by an imported module and applied to arguments.
    ///
    /// The qualifier is a *binding* made by an `import`, not a path: a
    /// module is reached as `io.`, never as `std.io.`
    /// (`docs/modules.md` §4).
    Name { name: Symbol, qualifier: Option<Symbol>, args: Vec<TypeId> },
    /// `&r T` and `&!r T` (`docs/linearity-and-effects.md` §5). The region is
    /// a name the parser does not resolve, exactly like a type's name.
    Ref { unique: bool, region: Symbol, inner: TypeId },
    /// `[T]` — a run of `T`s whose length is a runtime value.
    ///
    /// Unsized: it is what a reference points *at*, never a value on its
    /// own, so it appears as `&r [T]` or `&!r [T]` and nowhere else.
    Slice(TypeId),
    /// `(A, B)` — an anonymous aggregate with positional components
    /// (`docs/tuples.md`).
    ///
    /// Two or more, always: `(T)` is grouping and there is no `()`. A
    /// parenthesis at the *start* of a type can only open one of these,
    /// since `Ffi("libc")`'s parenthesis follows a name.
    Tuple(Vec<TypeId>),
    /// The `"libc"` in `Ffi("libc")` — a type indexed by a literal (§7.4).
    ///
    /// It is a *type*, not a value: `Ffi("libc")` and `Ffi("libm")` are two
    /// different types, which is how a narrowed capability differs from a
    /// wider one in a way the checker can see.
    Lit(String),
}

impl TypeExpr {
    /// The name written at the head, for a plain type. A reference has none:
    /// `&r T` names no type of its own, it points at one.
    pub fn head(&self) -> Option<Symbol> {
        match self {
            TypeExpr::Name { name, .. } => Some(*name),
            TypeExpr::Ref { .. } | TypeExpr::Lit(_) | TypeExpr::Slice(_) | TypeExpr::Tuple(_) => {
                None
            }
        }
    }

    pub fn args(&self) -> &[TypeId] {
        match self {
            // A tuple's components are its arguments for the purpose of a
            // traversal: they are the types written inside it, which is
            // what every caller of this wants.
            TypeExpr::Name { args, .. } | TypeExpr::Tuple(args) => args,
            TypeExpr::Ref { .. } | TypeExpr::Lit(_) | TypeExpr::Slice(_) => &[],
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnOp {
    Neg,
    Not,
    /// `*r` — read what a reference points at
    /// (`docs/reading-references.md` §3).
    ///
    /// Prefix, so it never collides with multiplication: the parser knows
    /// which position it is in, and `a * *b` is a product of `a` and what
    /// `b` points at.
    Deref,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BinOp {
    /// `&&` and `||` short-circuit: the right operand is not evaluated when
    /// the left already decides the answer.
    And,
    Or,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Expr {
    /// An integer literal, already parsed: the AST stores the value, not the
    /// spelling, so `007` and `7` are the same node.
    Int(i64),
    Bool(bool),
    /// A string literal, which exists only to be *read by the checker*.
    ///
    /// There are no runtime strings — those are M3 — and this never becomes
    /// a value: the only place one may appear is a narrowing call, where
    /// §7.4 requires a literal so the refinement is checkable where it is
    /// written. The checker refuses it anywhere else.
    Str(String),
    Name(Symbol),
    /// `Point { x: 1, y: 2 }` — fields in the order written, which need not be
    /// the order they were declared.
    StructLit {
        name: Symbol,
        qualifier: Option<Symbol>,
        fields: Vec<(Symbol, ExprId)>,
    },
    /// `p.x`
    Field {
        base: ExprId,
        name: Symbol,
    },
    /// `(a, b)` — an anonymous aggregate (`docs/tuples.md`).
    ///
    /// Two components or more. One would be grouping, which leaves no node
    /// behind, so the parser knows which it has by whether a comma follows
    /// the first expression.
    Tuple(Vec<ExprId>),
    /// `t.0` — a component by position (§3.1).
    ///
    /// Its own node rather than a `Field` with a numeric name, because a
    /// position is not a name: nothing interns it, and the checker reaches
    /// for it by index without a lookup.
    TupleField {
        base: ExprId,
        index: u32,
    },
    /// `Shape::Circle(3)`, or `Shape::Empty` with no arguments.
    ///
    /// Variants are always written qualified. Unqualified would mean two enums
    /// could not share a variant name, and `None` is exactly the name two
    /// enums want.
    Variant {
        enum_name: Symbol,
        qualifier: Option<Symbol>,
        variant: Symbol,
        args: Vec<ExprId>,
    },
    Unary {
        op: UnOp,
        operand: ExprId,
    },
    Binary {
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
    },
    Call {
        callee: Symbol,
        /// The module this call reaches into, if it was written `q.f(..)`
        /// (`docs/modules.md` §4). `None` is the ordinary case: a name in
        /// the caller's own module.
        qualifier: Option<Symbol>,
        args: Vec<ExprId>,
    },
    /// `s[i]` — one element of a slice, bounds-checked at runtime.
    ///
    /// Postfix, like `.field`, and parsed the same way: a primary followed
    /// by brackets. A type-argument list also uses brackets, but types and
    /// expressions are different positions, so nothing is ambiguous except
    /// the two arena builtins, which are a closed set of reserved names.
    Index {
        base: ExprId,
        index: ExprId,
    },
    /// `alloc[a](Node { value: 1 })` — allocate in an arena (§6).
    ///
    /// Its own node rather than a call, because the brackets name a *region*
    /// and nothing else in the language does that at a call site. The region
    /// is not inferred: an arena is chosen, never guessed.
    Alloc {
        region: Symbol,
        value: ExprId,
    },
    /// `alloc_slice[a](count, fill)` — a run of `count` copies of `fill` in
    /// an arena, handed back as `&!a [T]`.
    ///
    /// The length is a runtime value and the fill is evaluated once, which
    /// is what makes this a *slice* constructor rather than an array
    /// literal. Array literals would need a length in the type, and a
    /// length in the type is a second kind of generic parameter.
    AllocSlice {
        region: Symbol,
        count: ExprId,
        fill: ExprId,
    },
}

/// A brace-delimited sequence of statements.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Block {
    pub stmts: Vec<StmtId>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Stmt {
    /// `let x: int = e;` / `var x = e;` — the annotation is optional, and its
    /// absence means the checker infers.
    Let {
        name: Symbol,
        mutable: bool,
        ty: Option<TypeId>,
        value: ExprId,
    },
    /// `x = e;`
    /// `x = e;` and `r.f = e;` — the left side is a *place*, not just a name.
    ///
    /// Which expressions are places is the checker's question, not the
    /// parser's: it parses an expression and then sees an `=`, the same way
    /// it does not decide which names are types.
    Assign {
        place: ExprId,
        value: ExprId,
    },
    /// `let Point { x, y } = p;`
    ///
    /// Taking a value apart is one of the four ways to consume a linear one
    /// (§4.1): the whole is spent and the parts are produced, each subject to
    /// the rule in turn. Fields bind under their own names.
    Destructure {
        struct_name: Symbol,
        qualifier: Option<Symbol>,
        fields: Vec<Symbol>,
        value: ExprId,
    },
    /// `let (a, b) = t;` (`docs/tuples.md` §3.2).
    ///
    /// The same statement as [`Stmt::Destructure`] against a type that has
    /// no declaration, and the one pattern here that **names its own
    /// bindings**: a struct pattern inherits the field names, a tuple has
    /// none to inherit. That is why tuples close `sharing.md` §4's second
    /// gap as well as its first.
    DestructureTuple {
        names: Vec<Symbol>,
        value: ExprId,
    },
    /// `borrow x as &r in { .. }` / `borrow mut x as &!r in { .. }`.
    ///
    /// Introduces a region and binds a reference, both called `region`: the
    /// name is the region in a type and the reference in an expression, which
    /// is how §5 writes it (`borrow f as &r in { size(r) }`).
    ///
    /// A statement rather than an expression: lex-sys blocks are statement
    /// lists, so there is no tail expression for a value to come out of. §5's
    /// examples are illustrative, and this keeps `borrow` shaped like `if`
    /// and `while`.
    Borrow {
        value: Symbol,
        unique: bool,
        region: Symbol,
        body: Block,
    },
    /// `region a { .. }` — an arena (§6).
    ///
    /// The same shape as `borrow`, deliberately: it introduces a region that
    /// is a block, and what may not escape it is decided by the same
    /// occurs-check. An arena's lifetime and a borrow's lifetime are one
    /// mechanism, which is the section's claim.
    ///
    /// The region is written bare rather than as `&a`, because `&` is the
    /// reference constructor and there is nothing else a `region` could
    /// introduce.
    Region {
        region: Symbol,
        body: Block,
    },
    /// `e;` — the value is discarded.
    Expr(ExprId),
    If {
        cond: ExprId,
        then_block: Block,
        else_block: Option<Block>,
    },
    While {
        cond: ExprId,
        body: Block,
    },
    Match {
        scrutinee: ExprId,
        arms: Vec<MatchArm>,
    },
    Return(ExprId),
}

/// One label in an effect row, with the value it was narrowed to.
///
/// §7.4: "an effect label may carry a value, which is what makes
/// `[fs_write("/tmp/x")]` different from `[fs_write]`". The value is a
/// compile-time literal and never a runtime one — narrowing is checkable
/// structurally precisely because it is.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EffectLabel {
    pub name: Symbol,
    pub argument: Option<String>,
}

/// `extern fn name(params) -> [row] ret;` — a foreign signature (§8.4).
///
/// No body: the implementation is somebody else's, which is the whole point.
/// The declaration is the only place a foreign signature is written, so
/// there is exactly one place to get it wrong.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExternDecl {
    pub name: Symbol,
    pub regions: Vec<Symbol>,
    pub params: Vec<Param>,
    pub effects: Vec<EffectLabel>,
    pub ret: TypeId,
    /// The symbol the linker binds, which is the function's own name: a
    /// foreign function is named by what it is called out there.
    pub symbol: String,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Param {
    pub name: Symbol,
    pub ty: TypeId,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FnDecl {
    pub name: Symbol,
    /// A `val` bound per type parameter, parallel to `generics`
    /// (`docs/mode-polymorphism.md` §3.1).
    ///
    /// `None` is unbounded, which is checked as though the parameter were
    /// **`res`** -- the stronger obligation, so a body that passes is safe
    /// at every instantiation. There is no `res` bound: it would mean what
    /// unbounded already means (§3.2).
    pub bounds: Vec<Option<Mode>>,
    /// `pub` — reachable from another module (`docs/modules.md` §5).
    /// Never *safe*: §6 says a module is not a trust boundary.
    pub public: bool,
    pub generics: Vec<Symbol>,
    /// Region parameters, written `&r` in the same bracket list as the type
    /// parameters (§5.1). They are marked at the binder rather than inferred
    /// from use, so `fn f[T, &r]` says which is which without reading the
    /// parameter list — and an unused one is still unambiguous.
    pub regions: Vec<Symbol>,
    /// `where a <= b`, meaning `b` outlives `a` (§5.2). Each pair is
    /// `(inner, outer)` and both name region parameters.
    pub outlives: Vec<(Symbol, Symbol)>,
    pub params: Vec<Param>,
    /// The effect row, written between `->` and the return type
    /// (`docs/linearity-and-effects.md` §7). Labels in the order written; the
    /// checker canonicalises them, because *this* is the AST and the AST
    /// keeps what was written.
    ///
    /// Always present: `[]` is how a signature says pure, and §7.2 requires
    /// every signature to declare its row. An absent row would be an
    /// inferred one, which is the thing §7.2 exists to refuse.
    pub effects: Vec<EffectLabel>,
    pub ret: TypeId,
    pub body: Block,
}

/// A type's declared mode — how many times a value of it may be used.
///
/// `docs/linearity-and-effects.md` §3. Absent means inferred: a type is `res`
/// if any member is, and `val` otherwise.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Unrestricted: copyable, discardable, no obligations.
    Val,
    /// Linear: used exactly once, no implicit copy, no implicit discard.
    Res,
}

/// What an arm matches. M1 has variant patterns and a wildcard; literal and
/// nested patterns are later work.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Pattern {
    /// `_` — matches anything and binds nothing.
    Wildcard,
    /// `Shape::Rect(w, h)`. Each binding is a name, or `None` for `_`.
    Variant { enum_name: Symbol, variant: Symbol, bindings: Vec<Option<Symbol>> },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Block,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FieldDecl {
    pub name: Symbol,
    pub ty: TypeId,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StructDecl {
    pub name: Symbol,
    /// `pub` — reachable from another module (`docs/modules.md` §5).
    /// Never *safe*: §6 says a module is not a trust boundary.
    pub public: bool,
    /// `None` where the declaration did not say; the checker infers it.
    pub mode: Option<Mode>,
    /// Type parameters, in declaration order. `Type::Param(i)` refers to the
    /// `i`th of these.
    pub generics: Vec<Symbol>,
    pub fields: Vec<FieldDecl>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VariantDecl {
    pub name: Symbol,
    /// Positional payload types; empty for a variant that carries nothing.
    pub payload: Vec<TypeId>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EnumDecl {
    pub name: Symbol,
    /// `pub` — reachable from another module (`docs/modules.md` §5).
    /// Never *safe*: §6 says a module is not a trust boundary.
    pub public: bool,
    pub mode: Option<Mode>,
    pub generics: Vec<Symbol>,
    pub variants: Vec<VariantDecl>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Item {
    Fn(FnDecl),
    Extern(ExternDecl),
    Struct(StructDecl),
    Enum(EnumDecl),
}

/// `import a.b;` / `import a.b as c;` (`docs/modules.md` §4).
///
/// The binding is a **qualifier**, not a set of names: it says what `c.`
/// means, and nothing comes into scope unqualified.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Import {
    /// The module's path, segment by segment.
    pub path: Vec<Symbol>,
    /// What it is bound as: the last segment, or whatever `as` named.
    pub alias: Symbol,
    pub span: Span,
}

/// A namespace (`docs/modules.md` §3).
///
/// The root module has an empty path and needs no declaration, which is
/// why every program written before modules still compiles.
///
/// Imports live here rather than on a file because §3.2 lets two files
/// declare one module and share its namespace — and a namespace you share
/// while each half sees different names is two namespaces wearing one
/// name.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Module {
    pub path: Vec<Symbol>,
    pub imports: Vec<Import>,
}

impl Module {
    pub fn is_root(&self) -> bool {
        self.path.is_empty()
    }
}

/// A parsed compilation unit: three arenas, three span side tables, one
/// interner, and the item order the file had.
#[derive(Default, Debug)]
pub struct Ast {
    pub items: Vec<Item>,
    /// Every namespace the program declares, root first
    /// (`docs/modules.md` §3). Index 0 is always the root.
    pub modules: Vec<Module>,
    /// Which module each item belongs to, parallel to `items`. A side
    /// table rather than a field on `Item`, so nothing that walks items
    /// has to know modules exist unless it asks.
    item_modules: Vec<u32>,
    pub exprs: Vec<Expr>,
    pub stmts: Vec<Stmt>,
    pub types: Vec<TypeExpr>,
    pub symbols: Interner,

    item_spans: Vec<Span>,
    expr_spans: Vec<Span>,
    stmt_spans: Vec<Span>,
    type_spans: Vec<Span>,
}

/// Names the compiler provides, interned before the source is read.
///
/// A program never declares `World` or `Io` — they are the capabilities of
/// `docs/linearity-and-effects.md` §8 — so without this the checker would
/// have no symbol to resolve `main(world: World)` against.
///
/// Interning them first makes the table "prelude, then first appearance"
/// rather than "first appearance", which is still a deterministic function
/// of the source. Nothing downstream notices, because a hash encodes a
/// name's *text* and never its index (`docs/canonical-ast.md` §4.1).
pub const PRELUDE: &[&str] = &[
    "World", "Io", "Split", "io", "Ffi", "ffi", "L", "Fs", "fs", "P", "Heap", "heap", "Box", "B",
    "Args", "args",
];

impl Ast {
    /// An AST whose interner already knows the prelude's names.
    pub fn new() -> Self {
        let mut ast = Ast::default();
        for name in PRELUDE {
            ast.symbols.intern(name);
        }
        // The root is module 0 and is never declared: a file that says
        // nothing is in it (`docs/modules.md` §3).
        ast.modules.push(Module::default());
        ast
    }

    /// The module an item belongs to.
    pub fn module_of(&self, item: ItemId) -> u32 {
        self.item_modules[item.index()]
    }

    pub fn module(&self, index: u32) -> &Module {
        &self.modules[index as usize]
    }

    /// Which module a reference resolves *into*, from `from`, given the
    /// qualifier it was written with (`docs/modules.md` §4).
    ///
    /// Shared rather than implemented twice: `lex-sys-id` needs it to
    /// decide which declaration a call names, and `lex-sys-ir` needs it to
    /// check the same call. Two copies that disagreed would make a hash
    /// and its meaning drift apart, which is the one thing a
    /// content-addressed store cannot survive.
    ///
    /// `None` means the qualifier is not bound here, which is an error the
    /// caller reports -- this function has no opinion about programs.
    pub fn resolve_module(&self, from: u32, qualifier: Option<Symbol>) -> Option<u32> {
        let Some(qualifier) = qualifier else {
            return Some(from);
        };
        let import = self.modules[from as usize].imports.iter().find(|i| i.alias == qualifier)?;
        self.modules.iter().position(|m| m.path == import.path).map(|i| i as u32)
    }

    /// Find a module by its path, or declare it. Two files declaring one
    /// module get the same index, which is what makes §3.2 true.
    pub fn module_named(&mut self, path: &[Symbol]) -> u32 {
        if let Some(index) = self.modules.iter().position(|m| m.path == path) {
            return index as u32;
        }
        self.modules.push(Module { path: path.to_vec(), imports: Vec::new() });
        self.modules.len() as u32 - 1
    }

    pub fn push_expr(&mut self, expr: Expr, span: Span) -> ExprId {
        self.exprs.push(expr);
        self.expr_spans.push(span);
        ExprId(self.exprs.len() as u32 - 1)
    }

    pub fn push_stmt(&mut self, stmt: Stmt, span: Span) -> StmtId {
        self.stmts.push(stmt);
        self.stmt_spans.push(span);
        StmtId(self.stmts.len() as u32 - 1)
    }

    pub fn push_type(&mut self, ty: TypeExpr, span: Span) -> TypeId {
        self.types.push(ty);
        self.type_spans.push(span);
        TypeId(self.types.len() as u32 - 1)
    }

    /// Push an item into the root module. Every caller that predates
    /// `docs/modules.md` means this, and the root is where a file with no
    /// `module` declaration puts things.
    pub fn push_item(&mut self, item: Item, span: Span) -> ItemId {
        self.push_item_in(item, span, 0)
    }

    pub fn push_item_in(&mut self, item: Item, span: Span, module: u32) -> ItemId {
        self.items.push(item);
        self.item_spans.push(span);
        self.item_modules.push(module);
        ItemId(self.items.len() as u32 - 1)
    }

    pub fn expr(&self, id: ExprId) -> &Expr {
        &self.exprs[id.index()]
    }

    pub fn stmt(&self, id: StmtId) -> &Stmt {
        &self.stmts[id.index()]
    }

    pub fn item(&self, id: ItemId) -> &Item {
        &self.items[id.index()]
    }

    pub fn ty(&self, id: TypeId) -> &TypeExpr {
        &self.types[id.index()]
    }

    pub fn expr_span(&self, id: ExprId) -> Span {
        self.expr_spans[id.index()]
    }

    pub fn stmt_span(&self, id: StmtId) -> Span {
        self.stmt_spans[id.index()]
    }

    pub fn item_span(&self, id: ItemId) -> Span {
        self.item_spans[id.index()]
    }

    pub fn type_span(&self, id: TypeId) -> Span {
        self.type_spans[id.index()]
    }

    pub fn name_of(&self, sym: Symbol) -> &str {
        self.symbols.resolve(sym)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_is_stable_and_ordered_by_first_appearance() {
        let mut i = Interner::default();
        let a = i.intern("a");
        let b = i.intern("b");
        assert_eq!(a, Symbol(0));
        assert_eq!(b, Symbol(1));
        assert_eq!(i.intern("a"), a);
        assert_eq!(i.resolve(b), "b");
        assert_eq!(i.len(), 2);
    }

    #[test]
    fn spans_live_beside_nodes_not_inside_them() {
        let mut ast = Ast::default();
        let id = ast.push_expr(Expr::Int(7), Span::new(3, 4));
        assert_eq!(ast.expr(id), &Expr::Int(7));
        assert_eq!(ast.expr_span(id), Span::new(3, 4));
        // The node itself carries no offset, so it compares equal to the same
        // literal parsed from anywhere else.
        assert_eq!(ast.exprs[id.index()], Expr::Int(7));
    }
}
