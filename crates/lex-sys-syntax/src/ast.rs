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
pub struct TypeExpr {
    pub name: Symbol,
    pub args: Vec<TypeId>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnOp {
    Neg,
    Not,
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
    Name(Symbol),
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
        args: Vec<ExprId>,
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
    Assign {
        name: Symbol,
        value: ExprId,
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
    Return(ExprId),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Param {
    pub name: Symbol,
    pub ty: TypeId,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FnDecl {
    pub name: Symbol,
    pub params: Vec<Param>,
    pub ret: TypeId,
    pub body: Block,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Item {
    Fn(FnDecl),
}

/// A parsed compilation unit: three arenas, three span side tables, one
/// interner, and the item order the file had.
#[derive(Default, Debug)]
pub struct Ast {
    pub items: Vec<Item>,
    pub exprs: Vec<Expr>,
    pub stmts: Vec<Stmt>,
    pub types: Vec<TypeExpr>,
    pub symbols: Interner,

    item_spans: Vec<Span>,
    expr_spans: Vec<Span>,
    stmt_spans: Vec<Span>,
    type_spans: Vec<Span>,
}

impl Ast {
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

    pub fn push_item(&mut self, item: Item, span: Span) -> ItemId {
        self.items.push(item);
        self.item_spans.push(span);
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
