//! The canonical printer: an `Ast` back to text.
//!
//! This is the other direction of the content-addressed pipeline. The AST is
//! canonical by construction — whitespace, comments and redundant parentheses
//! never reach it (`docs/canonical-ast.md` §3) — so there is exactly one
//! reasonable rendering of any tree, and this produces it.
//!
//! Two properties, both tested rather than asserted:
//!
//! 1. **Identity-preserving.** Parsing the output gives the same `SigId`,
//!    `BodyId` and `TypeId` for every declaration as parsing the input did.
//!    That is what makes it *the* printer rather than *a* printer: it is the
//!    rendering step of a store that addresses code by hash.
//! 2. **Idempotent.** Printing the output again changes nothing, so the form
//!    is a fixed point rather than a direction of travel.
//!
//! It is deliberately **not** a `gofmt`. Comments are discarded by the lexer
//! so that formatting cannot change a content hash, which means a printer
//! built on the AST cannot preserve them — and a formatter that silently
//! deletes every comment in a file would be a bad trade. What this is for is
//! rendering a declaration fetched from the store, where there were never any
//! comments to lose.

use std::fmt::Write as _;

use crate::ast::{
    Ast, BinOp, Block, EffectLabel, EnumDecl, Expr, ExprId, ExternDecl, FnDecl, Item, ItemId, Mode,
    Module, Param, Pattern, Stmt, StmtId, StructDecl, Symbol, TypeExpr, TypeId, UnOp,
};

/// Render a whole unit.
pub fn print(ast: &Ast) -> String {
    let mut printer = Printer { ast, out: String::new(), depth: 0 };
    // Items grouped by module, in module order, each under its own
    // `module` declaration and imports (`docs/modules.md` §3). One `Ast`
    // may hold several files and therefore several modules, and the
    // canonical form is one text -- so the grouping is what makes
    // printing a fixed point when it does.
    let mut first = true;
    for index in 0..ast.modules.len() as u32 {
        let module = ast.module(index);
        let items: Vec<ItemId> = (0..ast.items.len() as u32)
            .map(ItemId)
            .filter(|id| ast.module_of(*id) == index)
            .collect();
        if items.is_empty() && module.imports.is_empty() {
            continue;
        }
        if !first {
            printer.out.push('\n');
        }
        first = false;
        printer.module_header(module);
        for (n, id) in items.iter().enumerate() {
            if n > 0 || !module.is_root() || !module.imports.is_empty() {
                printer.out.push('\n');
            }
            printer.item(&ast.items[id.index()]);
        }
    }
    printer.out
}

const INDENT: &str = "    ";

struct Printer<'a> {
    ast: &'a Ast,
    out: String,
    depth: usize,
}

impl Printer<'_> {
    /// `module a.b;` and the module's imports, in the order written.
    ///
    /// Import order is *not* canonicalised. Two imports differing only in
    /// order are the same namespace, so sorting them would be defensible
    /// -- but nothing hashes an import (`docs/modules.md` §2), so there is
    /// no identity to protect and no reason to move what the author
    /// wrote. `canonical-ast.md` §8 keeps the same gap open for a struct
    /// literal's field order, for the same reason.
    fn module_header(&mut self, module: &Module) {
        if !module.is_root() {
            let path: Vec<&str> = module.path.iter().map(|s| self.name(*s)).collect();
            let text = format!("module {};", path.join("."));
            self.line(&text);
        }
        for import in &module.imports {
            let path: Vec<&str> = import.path.iter().map(|s| self.name(*s)).collect();
            let last = *import.path.last().expect("a path has a last segment");
            let text = if last == import.alias {
                format!("import {};", path.join("."))
            } else {
                format!("import {} as {};", path.join("."), self.name(import.alias))
            };
            self.line(&text);
        }
    }

    fn line(&mut self, text: &str) {
        for _ in 0..self.depth {
            self.out.push_str(INDENT);
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn name(&self, symbol: Symbol) -> &str {
        self.ast.name_of(symbol)
    }

    /// `name`, or `q.name` where an import qualified it.
    fn qualified(&self, qualifier: Option<Symbol>, name: Symbol) -> String {
        match qualifier {
            Some(q) => format!("{}.{}", self.name(q), self.name(name)),
            None => self.name(name).to_owned(),
        }
    }

    // ---- items ---------------------------------------------------------

    fn item(&mut self, item: &Item) {
        match item {
            Item::Fn(decl) => self.fn_decl(decl),
            Item::Extern(decl) => self.extern_decl(decl),
            Item::Struct(decl) => self.struct_decl(decl),
            Item::Enum(decl) => self.enum_decl(decl),
        }
    }

    fn fn_decl(&mut self, decl: &FnDecl) {
        let header = format!(
            "{}fn {}{}({}) -> {} {} {{",
            visibility(decl.public),
            self.name(decl.name),
            self.declaration_params(&decl.generics, &decl.bounds, &decl.regions, &decl.outlives),
            self.params(&decl.params),
            self.effect_row(&decl.effects),
            self.ty(decl.ret),
        );
        self.line(&header);
        self.depth += 1;
        self.block_body(&decl.body);
        self.depth -= 1;
        self.line("}");
    }

    fn extern_decl(&mut self, decl: &ExternDecl) {
        let text = format!(
            "extern fn {}{}({}) -> {} {};",
            self.name(decl.name),
            self.declaration_params(&[], &[], &decl.regions, &[]),
            self.params(&decl.params),
            self.effect_row(&decl.effects),
            self.ty(decl.ret),
        );
        self.line(&text);
    }

    fn struct_decl(&mut self, decl: &StructDecl) {
        let header = format!(
            "{}{}struct {}{} {{",
            visibility(decl.public),
            mode_prefix(decl.mode),
            self.name(decl.name),
            self.declaration_params(&decl.generics, &decl.bounds, &[], &[])
        );
        self.line(&header);
        self.depth += 1;
        for field in &decl.fields {
            let text = format!("{}: {},", self.name(field.name), self.ty(field.ty));
            self.line(&text);
        }
        self.depth -= 1;
        self.line("}");
    }

    fn enum_decl(&mut self, decl: &EnumDecl) {
        let header = format!(
            "{}{}enum {}{} {{",
            visibility(decl.public),
            mode_prefix(decl.mode),
            self.name(decl.name),
            self.declaration_params(&decl.generics, &decl.bounds, &[], &[])
        );
        self.line(&header);
        self.depth += 1;
        for variant in &decl.variants {
            let payload = if variant.payload.is_empty() {
                String::new()
            } else {
                let types: Vec<String> = variant.payload.iter().map(|t| self.ty(*t)).collect();
                format!("({})", types.join(", "))
            };
            let text = format!("{}{payload},", self.name(variant.name));
            self.line(&text);
        }
        self.depth -= 1;
        self.line("}");
    }

    /// `[T, &r where a <= b]` — the bracket list a declaration takes, empty
    /// when it takes neither type nor region parameters.
    fn declaration_params(
        &self,
        generics: &[Symbol],
        bounds: &[Option<Mode>],
        regions: &[Symbol],
        outlives: &[(Symbol, Symbol)],
    ) -> String {
        if generics.is_empty() && regions.is_empty() {
            return String::new();
        }
        // `T`, or `T: val` (`docs/mode-polymorphism.md` §3.1). An unbounded
        // parameter prints bare, because that is what it is -- not `T: res`,
        // which does not exist (§3.2).
        let mut parts: Vec<String> = generics
            .iter()
            .enumerate()
            .map(|(i, g)| match bounds.get(i).copied().flatten() {
                Some(Mode::Val) => format!("{}: val", self.name(*g)),
                _ => self.name(*g).to_owned(),
            })
            .collect();
        parts.extend(regions.iter().map(|r| format!("&{}", self.name(*r))));
        let mut text = format!("[{}", parts.join(", "));
        if !outlives.is_empty() {
            let clauses: Vec<String> = outlives
                .iter()
                .map(|(inner, outer)| format!("{} <= {}", self.name(*inner), self.name(*outer)))
                .collect();
            let _ = write!(text, " where {}", clauses.join(", "));
        }
        text.push(']');
        text
    }

    fn params(&self, params: &[Param]) -> String {
        let written: Vec<String> =
            params.iter().map(|p| format!("{}: {}", self.name(p.name), self.ty(p.ty))).collect();
        written.join(", ")
    }

    /// `[]`, `[io]`, `[ffi("libc"), io]` — in the order written, because this
    /// is the AST and the AST keeps what was written. Canonicalising the row
    /// is the checker's job and happens to the *type*, not to the text.
    fn effect_row(&self, effects: &[EffectLabel]) -> String {
        let labels: Vec<String> = effects
            .iter()
            .map(|e| match &e.argument {
                Some(argument) => format!("{}(\"{argument}\")", self.name(e.name)),
                None => self.name(e.name).to_owned(),
            })
            .collect();
        format!("[{}]", labels.join(", "))
    }

    // ---- statements ----------------------------------------------------

    fn block_body(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.stmt(*stmt);
        }
    }

    fn stmt(&mut self, id: StmtId) {
        match self.ast.stmt(id) {
            Stmt::Let { name, mutable, ty, value } => {
                let keyword = if *mutable { "var" } else { "let" };
                let annotation = match ty {
                    Some(written) => format!(": {}", self.ty(*written)),
                    None => String::new(),
                };
                let text =
                    format!("{keyword} {}{annotation} = {};", self.name(*name), self.expr(*value));
                self.line(&text);
            }
            Stmt::Assign { place, value } => {
                let text = format!("{} = {};", self.expr(*place), self.expr(*value));
                self.line(&text);
            }
            Stmt::Destructure { struct_name, qualifier, fields, value } => {
                let names: Vec<&str> = fields.iter().map(|f| self.name(*f)).collect();
                let text = format!(
                    "let {} {{ {} }} = {};",
                    self.qualified(*qualifier, *struct_name),
                    names.join(", "),
                    self.expr(*value)
                );
                self.line(&text);
            }
            Stmt::DestructureTuple { names, value } => {
                let written: Vec<&str> = names.iter().map(|n| self.name(*n)).collect();
                let text = format!("let ({}) = {};", written.join(", "), self.expr(*value));
                self.line(&text);
            }
            Stmt::Borrow { value, unique, region, body } => {
                let text = format!(
                    "borrow {}{} as &{}{} in {{",
                    if *unique { "mut " } else { "" },
                    self.name(*value),
                    if *unique { "!" } else { "" },
                    self.name(*region)
                );
                self.line(&text);
                self.nested(body);
            }
            Stmt::Region { region, body } => {
                let text = format!("region {} {{", self.name(*region));
                self.line(&text);
                self.nested(body);
            }
            Stmt::Expr(value) => {
                let text = format!("{};", self.expr(*value));
                self.line(&text);
            }
            Stmt::If { cond, then_block, else_block } => {
                let text = format!("if {} {{", self.condition(*cond));
                self.line(&text);
                self.depth += 1;
                self.block_body(then_block);
                self.depth -= 1;
                match else_block {
                    Some(body) => {
                        self.line("} else {");
                        self.nested(body);
                    }
                    None => self.line("}"),
                }
            }
            Stmt::While { cond, body } => {
                let text = format!("while {} {{", self.condition(*cond));
                self.line(&text);
                self.nested(body);
            }
            Stmt::Match { scrutinee, arms } => {
                let text = format!("match {} {{", self.condition(*scrutinee));
                self.line(&text);
                self.depth += 1;
                for arm in arms {
                    let pattern = match &arm.pattern {
                        Pattern::Wildcard => "_".to_owned(),
                        Pattern::Variant { enum_name, qualifier, variant, bindings } => {
                            let head = format!(
                                "{}::{}",
                                self.qualified(*qualifier, *enum_name),
                                self.name(*variant)
                            );
                            if bindings.is_empty() {
                                head
                            } else {
                                let names: Vec<&str> = bindings
                                    .iter()
                                    .map(|b| b.map_or("_", |name| self.name(name)))
                                    .collect();
                                format!("{head}({})", names.join(", "))
                            }
                        }
                    };
                    let text = format!("{pattern} => {{");
                    self.line(&text);
                    self.nested(&arm.body);
                }
                self.depth -= 1;
                self.line("}");
            }
            Stmt::Return(value) => {
                let text = format!("return {};", self.expr(*value));
                self.line(&text);
            }
            Stmt::Defer(value) => {
                let text = format!("defer {};", self.expr(*value));
                self.line(&text);
            }
        }
    }

    /// A block that has already had its opening brace written.
    fn nested(&mut self, block: &Block) {
        self.depth += 1;
        self.block_body(block);
        self.depth -= 1;
        self.line("}");
    }

    // ---- types ---------------------------------------------------------

    fn ty(&self, id: TypeId) -> String {
        match self.ast.ty(id) {
            TypeExpr::Name { name, qualifier, args } => {
                let head = &self.qualified(*qualifier, *name);
                match args.split_first() {
                    // `Ffi("libc")`: a literal argument is written in
                    // parentheses, because it indexes by a value (§7.4).
                    Some((first, rest))
                        if rest.is_empty() && matches!(self.ast.ty(*first), TypeExpr::Lit(_)) =>
                    {
                        format!("{head}({})", self.ty(*first))
                    }
                    None => head.to_owned(),
                    _ => {
                        let written: Vec<String> = args.iter().map(|a| self.ty(*a)).collect();
                        format!("{head}[{}]", written.join(", "))
                    }
                }
            }
            TypeExpr::Ref { unique, region, inner } => format!(
                "&{}{} {}",
                if *unique { "!" } else { "" },
                self.name(*region),
                self.ty(*inner)
            ),
            TypeExpr::Slice(inner) => format!("[{}]", self.ty(*inner)),
            TypeExpr::Tuple(parts) => {
                let written: Vec<String> = parts.iter().map(|p| self.ty(*p)).collect();
                format!("({})", written.join(", "))
            }
            TypeExpr::Lit(text) => format!("\"{}\"", escape(text)),
        }
    }

    // ---- expressions ---------------------------------------------------

    /// An expression in a position followed by a block: `if`, `while` and
    /// `match`.
    ///
    /// A struct literal's braces would be taken for the block there, so the
    /// parser refuses a bare one and the source has to parenthesise. The
    /// parentheses leave no node behind, so the printer has to know to put
    /// them back -- this is the one place the canonical form is decided by
    /// the grammar rather than by the tree.
    fn condition(&self, id: ExprId) -> String {
        let text = self.expr(id);
        if self.has_bare_struct_literal(id) { format!("({text})") } else { text }
    }

    /// Does a struct literal appear where a block could follow it?
    ///
    /// Anything inside brackets is safe: the parser allows literals again
    /// there, because a bracket already says where the expression ends.
    fn has_bare_struct_literal(&self, id: ExprId) -> bool {
        match self.ast.expr(id) {
            Expr::StructLit { .. } => true,
            Expr::Binary { lhs, rhs, .. } => {
                self.has_bare_struct_literal(*lhs) || self.has_bare_struct_literal(*rhs)
            }
            Expr::Unary { operand, .. } => self.has_bare_struct_literal(*operand),
            // The base of `x.f` or `x[i]` is still out in the open; the
            // index is not.
            Expr::Field { base, .. } | Expr::Index { base, .. } => {
                self.has_bare_struct_literal(*base)
            }
            _ => false,
        }
    }

    /// An expression at the outermost precedence, where nothing needs
    /// parenthesising.
    fn expr(&self, id: ExprId) -> String {
        self.expr_at(id, 0)
    }

    /// `level` is the binding power of the context: anything that binds more
    /// loosely has to be wrapped.
    ///
    /// The AST holds no parentheses at all — grouping leaves no node, which
    /// is what makes `(a + b) * c` and `(((a + b)) * c)` one tree — so the
    /// printer puts back exactly the ones the text needs and no others.
    /// Getting that wrong shows up as a changed hash, which is what the
    /// round-trip test is for.
    fn expr_at(&self, id: ExprId, level: u8) -> String {
        match self.ast.expr(id) {
            Expr::Int(value) => value.to_string(),
            Expr::Bool(value) => value.to_string(),
            Expr::Str(text) => format!("\"{}\"", escape(text)),
            Expr::Float(bits) => float_literal(*bits),
            Expr::Name(name) => self.name(*name).to_owned(),
            Expr::StructLit { name, qualifier, fields } => {
                let head = self.qualified(*qualifier, *name);
                let written: Vec<String> = fields
                    .iter()
                    .map(|(field, value)| format!("{}: {}", self.name(*field), self.expr(*value)))
                    .collect();
                if written.is_empty() {
                    format!("{head} {{ }}")
                } else {
                    format!("{head} {{ {} }}", written.join(", "))
                }
            }
            Expr::Field { base, name } => {
                format!("{}.{}", self.expr_at(*base, POSTFIX), self.name(*name))
            }
            // A tuple's own parentheses bind it, so it prints at any level
            // without needing the precedence wrapper.
            Expr::Tuple(parts) => {
                let written: Vec<String> = parts.iter().map(|p| self.expr(*p)).collect();
                format!("({})", written.join(", "))
            }
            Expr::TupleField { base, index } => {
                format!("{}.{index}", self.expr_at(*base, POSTFIX))
            }
            Expr::Index { base, index } => {
                format!("{}[{}]", self.expr_at(*base, POSTFIX), self.expr(*index))
            }
            Expr::Slice { base, start, end } => {
                format!(
                    "{}[{}..{}]",
                    self.expr_at(*base, POSTFIX),
                    self.expr(*start),
                    self.expr(*end)
                )
            }
            Expr::Variant { enum_name, qualifier, variant, args } => {
                let head =
                    format!("{}::{}", self.qualified(*qualifier, *enum_name), self.name(*variant));
                if args.is_empty() {
                    head
                } else {
                    let written: Vec<String> = args.iter().map(|a| self.expr(*a)).collect();
                    format!("{head}({})", written.join(", "))
                }
            }
            Expr::Call { callee, qualifier, args } => {
                let written: Vec<String> = args.iter().map(|a| self.expr(*a)).collect();
                format!("{}({})", self.qualified(*qualifier, *callee), written.join(", "))
            }
            Expr::Alloc { region, value } => {
                format!("alloc[{}]({})", self.name(*region), self.expr(*value))
            }
            Expr::AllocSlice { region, count, fill } => format!(
                "alloc_slice[{}]({}, {})",
                self.name(*region),
                self.expr(*count),
                self.expr(*fill)
            ),
            Expr::Unary { op, operand } => {
                let symbol = match op {
                    UnOp::Neg => "-",
                    UnOp::Not => "!",
                    UnOp::Deref => "*",
                    UnOp::BitNot => "~",
                };
                // `-` immediately before an integer *token* is one literal,
                // not a negation of one -- that is how
                // `-9223372036854775808` is writable at all. So a negation
                // whose operand is a literal has to keep the two apart, or
                // `-(5)` would print as `-5` and come back as a different
                // tree. Whitespace will not do it: the parser sees tokens.
                let inner = self.expr_at(*operand, UNARY);
                let text = match (op, self.ast.expr(*operand)) {
                    (UnOp::Neg, Expr::Int(_)) => format!("-({inner})"),
                    _ => format!("{symbol}{inner}"),
                };
                parenthesise(text, level, UNARY)
            }
            Expr::Binary { op, lhs, rhs } => {
                let power = binding_power(*op);
                // Left-associative throughout, so the right operand needs one
                // more than the operator's own power to stay unwrapped.
                let text = format!(
                    "{} {} {}",
                    self.expr_at(*lhs, power),
                    operator(*op),
                    self.expr_at(*rhs, power + 1)
                );
                parenthesise(text, level, power)
            }
        }
    }
}

/// Binding powers, loosest first. They only have to *order* the same way the
/// parser's do; the numbers themselves never leave this file.
const UNARY: u8 = 9;
const POSTFIX: u8 = 10;

fn binding_power(op: BinOp) -> u8 {
    match op {
        BinOp::Or => 1,
        BinOp::And => 2,
        BinOp::Eq | BinOp::Ne => 3,
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => 4,
        // `docs/bitwise.md` §5: tighter than comparison, looser than `+`.
        BinOp::BitOr => 5,
        BinOp::BitXor => 6,
        BinOp::BitAnd => 7,
        BinOp::Shl | BinOp::Shr => 8,
        BinOp::Add | BinOp::Sub => 9,
        BinOp::Mul | BinOp::Div | BinOp::Rem => 10,
    }
}

fn operator(op: BinOp) -> &'static str {
    match op {
        BinOp::Or => "||",
        BinOp::And => "&&",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
    }
}

fn parenthesise(text: String, context: u8, own: u8) -> String {
    if own < context { format!("({text})") } else { text }
}

/// Render a float literal so it parses back to the same bits.
///
/// Rust's `Display` for `f64` is the shortest decimal that round-trips,
/// which is exactly the contract a canonical form needs. It prints `1`
/// for `1.0` though, and `1` is an *integer* literal here — so a value
/// with no `.` and no `e` gets `.0` appended. The sign is handled by the
/// parser, which reads `-1.5` as one literal
/// (`docs/floating-point.md` §1).
fn float_literal(bits: u64) -> String {
    let value = f64::from_bits(bits);
    let rendered = format!("{value}");
    if rendered.contains(['.', 'e', 'E']) { rendered } else { format!("{rendered}.0") }
}

/// Put a literal's escapes back.
///
/// The AST holds a literal's *bytes*, with escapes already resolved
/// (`canonical-ast.md` §3 keeps values, not spellings), so printing the
/// text raw would put a real newline inside quotes — which does not
/// reparse, because a literal may not span lines. The six escapes §4 of
/// `docs/strings.md` admits are exactly the six that have to come back.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\0' => out.push_str("\\0"),
            other => out.push(other),
        }
    }
    out
}

/// `docs/modules.md` §5. Written before the mode, so a declaration reads
/// "public, and a resource" in that order.
fn visibility(public: bool) -> &'static str {
    if public { "pub " } else { "" }
}

fn mode_prefix(mode: Option<Mode>) -> &'static str {
    match mode {
        Some(Mode::Res) => "res ",
        Some(Mode::Val) => "val ",
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    /// Print, reparse, print again — and require both the text and the tree
    /// to settle. The tree comparison is the real one: `Ast`'s `PartialEq`
    /// covers the nodes, and the nodes are what a hash is taken over.
    fn round_trip(src: &str) -> String {
        let ast = parse(src).expect("should parse");
        let printed = print(&ast);
        let reparsed = parse(&printed)
            .unwrap_or_else(|d| panic!("printed output does not parse: {}\n{printed}", d.message));
        assert_eq!(ast.exprs, reparsed.exprs, "the expressions changed\n{printed}");
        assert_eq!(ast.stmts, reparsed.stmts, "the statements changed\n{printed}");
        assert_eq!(ast.items, reparsed.items, "the items changed\n{printed}");
        assert_eq!(printed, print(&reparsed), "printing is not a fixed point");
        printed
    }

    /// Wrap an expression in the smallest program that holds one.
    fn in_fn(expr: &str) -> String {
        format!("fn f(a: int, b: int, c: int) -> [] int {{ return {expr}; }}")
    }

    fn printed_expr(expr: &str) -> String {
        let text = round_trip(&in_fn(expr));
        let line = text.lines().nth(1).expect("the body").trim().to_owned();
        line.trim_start_matches("return ").trim_end_matches(';').to_owned()
    }

    #[test]
    fn precedence_parentheses_are_put_back_exactly_where_they_are_needed() {
        // The AST holds no parentheses -- grouping leaves no node -- so the
        // printer works these out from binding powers alone.
        assert_eq!(printed_expr("(a + b) * c"), "(a + b) * c");
        assert_eq!(printed_expr("a + b * c"), "a + b * c");
        assert_eq!(printed_expr("a * b + c"), "a * b + c");
        assert_eq!(printed_expr("(a * b) + c"), "a * b + c");
    }

    #[test]
    fn left_associativity_survives_the_round_trip() {
        // `a - (b - c)` is not `a - b - c`, and the printer is the only
        // thing standing between them.
        assert_eq!(printed_expr("a - b - c"), "a - b - c");
        assert_eq!(printed_expr("a - (b - c)"), "a - (b - c)");
        assert_eq!(printed_expr("a / (b / c)"), "a / (b / c)");
        assert_eq!(printed_expr("a % (b % c)"), "a % (b % c)");
    }

    #[test]
    fn a_negated_literal_keeps_its_parentheses() {
        // `-` immediately before an integer token is one literal, which is
        // how `-9223372036854775808` is writable. So a *negation* of a
        // literal has to stay visibly apart from one, or it would come back
        // as a different node.
        assert_eq!(printed_expr("-(5)"), "-(5)");
        assert_eq!(printed_expr("-5"), "-5");
        assert_eq!(printed_expr("a - -5"), "a - -5");
        // Only the operand that *is* a literal needs the parentheses: the
        // outer negation's operand is another negation, and `--(5)` lexes
        // as two minus signs because there is no `--` token.
        assert_eq!(printed_expr("-(-(5))"), "--(5)");
    }

    #[test]
    fn a_literals_escapes_come_back() {
        // The AST holds bytes, not spellings, so printing has to put the
        // escapes back or the output does not reparse -- a literal may not
        // span lines, and a raw newline inside quotes is exactly that.
        assert_eq!(printed_expr("\"a\\nb\""), "\"a\\nb\"");
        assert_eq!(printed_expr("\"tab\\there\""), "\"tab\\there\"");
        assert_eq!(printed_expr("\"C:\\\\path\""), "\"C:\\\\path\"");
        assert_eq!(printed_expr("\"say \\\"hi\\\"\""), "\"say \\\"hi\\\"\"");
        assert_eq!(printed_expr("\"nul\\0end\""), "\"nul\\0end\"");
        // `\r` is the sixth, added by `examples/serve/` rather than by a
        // list: a carriage return that printed as itself would reparse as a
        // literal spanning a line, which is refused.
        assert_eq!(printed_expr("\"line\\r\\n\""), "\"line\\r\\n\"");
    }

    #[test]
    fn a_struct_literal_before_a_block_is_parenthesised() {
        // The one place the canonical form is decided by the grammar rather
        // than by the tree: a bare literal's braces would be taken for the
        // block.
        round_trip(
            "struct P { x: int } \
             fn f() -> [] int { if (P { x: 1 }).x == 1 { return 0; } return 1; }",
        );
        round_trip(
            "struct P { x: int } \
             fn f() -> [] int { match (P { x: 1 }) { _ => { return 0; } } }",
        );
        round_trip(
            "struct P { x: int } \
             fn f() -> [] int { while (P { x: 1 }).x == 0 { return 0; } return 1; }",
        );
        // And a literal that is already inside brackets needs nothing added.
        round_trip(
            "struct P { x: int } fn g(p: P) -> [] int { return 0; } \
             fn f() -> [] int { if g(P { x: 1 }) == 0 { return 0; } return 1; }",
        );
    }

    #[test]
    fn short_circuit_operators_nest_the_way_they_were_written() {
        assert_eq!(printed_expr("a == 1 || b == 2 && c == 3"), "a == 1 || b == 2 && c == 3");
        assert_eq!(printed_expr("(a == 1 || b == 2) && c == 3"), "(a == 1 || b == 2) && c == 3");
        assert_eq!(printed_expr("!(a == 1 && b == 2)"), "!(a == 1 && b == 2)");
    }

    #[test]
    fn every_declaration_shape_survives() {
        round_trip(
            "res struct Ticket { serial: int } \
             val struct Pair[A, B] { left: A, right: B } \
             enum Opt[T] { None, Some(T), Both(T, T) } \
             extern fn labs[&f](ffi: &f Ffi(\"libc\"), n: int) -> [ffi(\"libc\")] int; \
             fn generic[T, &r where r <= r](x: &r T, y: [int]) -> [io, fs] int { return 0; }",
        );
    }

    #[test]
    fn every_statement_shape_survives() {
        round_trip(
            "struct P { x: int, y: int } \
             res struct T { n: int } \
             fn take(t: T) -> [] int { let T { n } = t; return n; } \
             fn f(t: T) -> [] int { \
                 var p = P { x: 1, y: 2 }; \
                 let q: P = p; \
                 p.x = 3; \
                 borrow p as &r in { let n = r.x; } \
                 borrow mut p as &!w in { w.y = 4; } \
                 region a { let cell = alloc[a](P { x: 0, y: 0 }); \
                            let xs = alloc_slice[a](2, 0); xs[0] = 1; } \
                 while p.x < 10 { p.x = p.x + 1; } \
                 if p.x == 10 { take(t); } else { take(t); } \
                 return 0; \
             }",
        );
    }

    #[test]
    fn a_match_keeps_its_patterns() {
        round_trip(
            "enum Shape { Empty, Circle(int), Rect(int, int) } \
             fn area(s: Shape) -> [] int { \
                 match s { \
                     Shape::Empty => { return 0; } \
                     Shape::Circle(r) => { return r * r; } \
                     Shape::Rect(w, _) => { return w; } \
                 } \
             }",
        );
    }

    #[test]
    fn the_output_reads_as_canonical_source() {
        // Not a hash property -- just that the shape is the one the repo is
        // written in, so a printed declaration sits beside a hand-written
        // one without looking foreign.
        let text = round_trip("fn f(a: int) -> [] int { if a > 0 { return 1; } return 0; }");
        assert_eq!(
            text,
            "fn f(a: int) -> [] int {\n    if a > 0 {\n        return 1;\n    }\n    return 0;\n}\n"
        );
    }
}
