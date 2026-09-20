//! Canonical encoding and content-addressed per-unit identity.
//!
//! `docs/canonical-ast.md` is the specification; this is the implementation,
//! and the two are meant to be read together. The short version:
//!
//! * A **unit** is one top-level declaration. It hashes alone, so its identity
//!   does not depend on what sits beside it in the file.
//! * A function has **two** identities. `SigId` covers what a caller depends
//!   on; `BodyId` covers the implementation. Rewriting a body leaves every
//!   caller's identity untouched.
//! * A body refers to its callees by `SigId`, and a `SigId` never depends on a
//!   body, so mutual recursion produces no cycle.
//! * Bodies hash up to alpha-equivalence: renaming a local changes nothing.
//!
//! Nothing consumes these hashes yet. They exist because M0 claimed the AST
//! was built for canonicalisation, and until something hashed it that was an
//! assertion rather than a fact.

use std::collections::HashMap;

use lex_sys_syntax::ast::{
    Ast, BinOp, Block, EffectLabel, EnumDecl, Expr, ExprId, ExternDecl, FnDecl, Item, Stmt, StmtId,
    StructDecl, Symbol, TypeExpr, TypeId, UnOp,
};

/// A 32-byte content hash.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hash([u8; 32]);

impl Hash {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Full lowercase hex. Comparison always uses this, never [`Hash::short`].
    pub fn to_hex(self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// The first 16 hex characters, for display only.
    pub fn short(self) -> String {
        self.to_hex()[..16].to_owned()
    }
}

impl std::fmt::Debug for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.short())
    }
}

impl std::fmt::Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Domain tags, so a signature's bytes can never be read as a body's.
///
/// The `v1` is not decoration: when a rule in `docs/canonical-ast.md` changes,
/// this changes with it, and old hashes stay readable as what they were.
const DOMAIN_SIG: &str = "lex-sys.sig.v1";
const DOMAIN_BODY: &str = "lex-sys.body.v1";
const DOMAIN_TYPE: &str = "lex-sys.type.v1";

/// Node tags. Explicit numbers, never reused — see `docs/canonical-ast.md` §8
/// for why these are not yet frozen across releases.
mod tag {
    pub const INT: u8 = 0x01;
    pub const BOOL: u8 = 0x02;
    pub const LOCAL: u8 = 0x03;
    pub const FREE: u8 = 0x04;
    pub const STRUCT_LIT: u8 = 0x05;
    pub const FIELD: u8 = 0x06;
    pub const VARIANT: u8 = 0x07;
    pub const UNARY: u8 = 0x08;
    pub const BINARY: u8 = 0x09;
    pub const CALL: u8 = 0x0a;

    pub const LET: u8 = 0x20;
    pub const ASSIGN: u8 = 0x21;
    pub const EXPR_STMT: u8 = 0x22;
    pub const IF: u8 = 0x23;
    pub const WHILE: u8 = 0x24;
    pub const MATCH: u8 = 0x25;
    pub const RETURN: u8 = 0x26;
    pub const DESTRUCTURE: u8 = 0x27;

    pub const TYPE_NAME: u8 = 0x40;
    pub const PATTERN_WILDCARD: u8 = 0x41;
    pub const PATTERN_VARIANT: u8 = 0x42;
    /// `&r T`; the `!` of a unique reference rides the following `bool`.
    pub const TYPE_REF: u8 = 0x43;
    pub const BORROW: u8 = 0x44;

    pub const STRUCT_DECL: u8 = 0x60;
    pub const ENUM_DECL: u8 = 0x61;

    /// A declaration's mode (`docs/linearity-and-effects.md` §3). An absent
    /// annotation and a written `val` encode the same, because on a type
    /// whose members are all `val` the word asserts exactly what absence
    /// already checks — and a `val` that was not true never reaches here.
    pub const MODE_VAL: u8 = 0x62;
    pub const MODE_RES: u8 = 0x63;
    /// An effect row (`docs/linearity-and-effects.md` §7.1). Sorted and
    /// deduplicated before it is encoded, which is what "a canonical order
    /// makes an effect row hashable" means in bytes.
    pub const EFFECTS: u8 = 0x64;
    /// A foreign declaration (`docs/linearity-and-effects.md` §8.4).
    pub const EXTERN_DECL: u8 = 0x65;
    /// A string literal, in the only two places one may appear: a narrowing
    /// argument and a type indexed by a literal.
    pub const STR: u8 = 0x66;
    pub const REGION: u8 = 0x67;
    pub const ALLOC: u8 = 0x68;
    pub const ALLOC_SLICE: u8 = 0x69;
    pub const INDEX: u8 = 0x6a;
    pub const TYPE_SLICE: u8 = 0x6b;

    /// The tag for a declared mode. Written out rather than cast from the
    /// enum, so adding a mode cannot silently renumber the others.
    pub fn mode_tag(mode: Option<lex_sys_syntax::ast::Mode>) -> u8 {
        use lex_sys_syntax::ast::Mode;
        match mode {
            None | Some(Mode::Val) => MODE_VAL,
            Some(Mode::Res) => MODE_RES,
        }
    }

    pub const NONE: u8 = 0x00;
    pub const SOME: u8 = 0x01;
}

/// Operator tags, written out rather than taken from the enum's discriminant.
///
/// `op as u8` would have worked and would have been a trap: the discriminant
/// is declaration order, and `&&`/`||` were once added to the *front* of
/// `BinOp`. That would have silently renumbered every other operator and moved
/// every hash containing one.
fn unary_tag(op: UnOp) -> u8 {
    match op {
        UnOp::Neg => 0x01,
        UnOp::Not => 0x02,
        // Appended, never inserted: a new tag takes the next number so that
        // every hash containing `-` or `!` stays where it was.
        UnOp::Deref => 0x03,
    }
}

fn binary_tag(op: BinOp) -> u8 {
    match op {
        BinOp::Add => 0x01,
        BinOp::Sub => 0x02,
        BinOp::Mul => 0x03,
        BinOp::Div => 0x04,
        BinOp::Rem => 0x05,
        BinOp::Eq => 0x06,
        BinOp::Ne => 0x07,
        BinOp::Lt => 0x08,
        BinOp::Le => 0x09,
        BinOp::Gt => 0x0a,
        BinOp::Ge => 0x0b,
        BinOp::And => 0x0c,
        BinOp::Or => 0x0d,
    }
}

/// A canonical byte string under construction.
///
/// Every method here is one of the encoding rules in `docs/canonical-ast.md`
/// §4: fixed-width integers, length-prefixed strings and sequences, nested
/// hashes as their raw bytes.
#[derive(Default)]
struct Encoder {
    bytes: Vec<u8>,
}

impl Encoder {
    fn tag(&mut self, tag: u8) -> &mut Self {
        self.bytes.push(tag);
        self
    }

    fn u32(&mut self, value: u32) -> &mut Self {
        self.bytes.extend_from_slice(&value.to_le_bytes());
        self
    }

    fn i64(&mut self, value: i64) -> &mut Self {
        self.bytes.extend_from_slice(&value.to_le_bytes());
        self
    }

    fn bool(&mut self, value: bool) -> &mut Self {
        self.bytes.push(u8::from(value));
        self
    }

    /// Length-prefixed UTF-8. Never null-terminated, so a name cannot smuggle
    /// a delimiter.
    fn str(&mut self, value: &str) -> &mut Self {
        self.u32(value.len() as u32);
        self.bytes.extend_from_slice(value.as_bytes());
        self
    }

    fn len(&mut self, count: usize) -> &mut Self {
        self.u32(count as u32)
    }

    fn hash(&mut self, hash: Hash) -> &mut Self {
        self.bytes.extend_from_slice(hash.as_bytes());
        self
    }

    fn finish(self, domain: &str) -> Hash {
        let mut hasher = blake3::Hasher::new();
        hasher.update(domain.as_bytes());
        hasher.update(&self.bytes);
        Hash(*hasher.finalize().as_bytes())
    }
}

/// The identities of one compilation unit.
#[derive(Clone, Debug, Default)]
pub struct Identities {
    /// Functions, in declaration order.
    pub functions: Vec<FunctionId>,
    /// Structs and enums, in declaration order.
    pub types: Vec<TypeDeclId>,
}

#[derive(Clone, Debug)]
pub struct FunctionId {
    pub name: String,
    /// What a caller depends on, and all it depends on.
    pub sig: Hash,
    /// The implementation.
    pub body: Hash,
}

#[derive(Clone, Debug)]
pub struct TypeDeclId {
    pub name: String,
    pub id: Hash,
}

impl Identities {
    pub fn function(&self, name: &str) -> Option<&FunctionId> {
        self.functions.iter().find(|f| f.name == name)
    }

    pub fn type_decl(&self, name: &str) -> Option<&TypeDeclId> {
        self.types.iter().find(|t| t.name == name)
    }
}

/// Compute every unit's identity.
///
/// Signatures and type declarations are hashed first, because a body refers to
/// its callees by `SigId` and to types by `TypeId`. That ordering is what
/// keeps the graph acyclic even when the source is not.
pub fn identify(ast: &Ast) -> Identities {
    let mut type_ids: HashMap<Symbol, Hash> = HashMap::new();
    for item in &ast.items {
        match item {
            Item::Struct(decl) => {
                type_ids.insert(decl.name, hash_struct(ast, decl));
            }
            Item::Enum(decl) => {
                type_ids.insert(decl.name, hash_enum(ast, decl));
            }
            Item::Fn(_) | Item::Extern(_) => {}
        }
    }

    let mut sig_ids: HashMap<Symbol, Hash> = HashMap::new();
    for item in &ast.items {
        match item {
            Item::Fn(decl) => {
                sig_ids.insert(decl.name, hash_signature(ast, decl, &type_ids));
            }
            // A foreign declaration is a signature and nothing else, so it
            // has one identity rather than two. A caller depends on it the
            // same way it depends on any other signature.
            Item::Extern(decl) => {
                sig_ids.insert(decl.name, hash_extern(ast, decl, &type_ids));
            }
            _ => {}
        }
    }

    let mut identities = Identities::default();
    for item in &ast.items {
        match item {
            Item::Fn(decl) => identities.functions.push(FunctionId {
                name: ast.name_of(decl.name).to_owned(),
                sig: sig_ids[&decl.name],
                body: hash_body(ast, decl, &sig_ids, &type_ids),
            }),
            // A foreign function has no body, so its two identities are the
            // same hash: there is nothing to rewrite that a caller could
            // fail to notice.
            Item::Extern(decl) => identities.functions.push(FunctionId {
                name: ast.name_of(decl.name).to_owned(),
                sig: sig_ids[&decl.name],
                body: sig_ids[&decl.name],
            }),
            Item::Struct(decl) => identities.types.push(TypeDeclId {
                name: ast.name_of(decl.name).to_owned(),
                id: type_ids[&decl.name],
            }),
            Item::Enum(decl) => identities.types.push(TypeDeclId {
                name: ast.name_of(decl.name).to_owned(),
                id: type_ids[&decl.name],
            }),
        }
    }
    identities
}

/// A written type, encoded structurally.
///
/// A name that belongs to a declared type contributes that declaration's hash,
/// so a signature mentioning `Point` changes when `Point` does. Anything else —
/// `int`, or a generic parameter — contributes its text.
fn encode_type(
    ast: &Ast,
    encoder: &mut Encoder,
    id: TypeId,
    type_ids: &HashMap<Symbol, Hash>,
    generics: &[Symbol],
    regions: &[Symbol],
) {
    let written: &TypeExpr = ast.ty(id);

    // A region is positional for the same reason a generic parameter is:
    // `fn len[&r](s: &r Bytes)` and `fn len[&q](s: &q Bytes)` are one
    // signature, and no caller can tell them apart.
    if let TypeExpr::Ref { unique, region, inner } = written {
        let (unique, region, inner) = (*unique, *region, *inner);
        encoder.tag(tag::TYPE_REF).bool(unique);
        match regions.iter().rposition(|r| *r == region) {
            Some(index) => {
                encoder.tag(tag::LOCAL).u32(index as u32);
            }
            None => {
                encoder.tag(tag::NONE).str(ast.name_of(region));
            }
        }
        encode_type(ast, encoder, inner, type_ids, generics, regions);
        return;
    }

    // `[T]`: a shape rather than a name, so it gets its own tag instead of
    // being a `Name` that no declaration could ever match.
    if let TypeExpr::Slice(inner) = written {
        let inner = *inner;
        encoder.tag(tag::TYPE_SLICE);
        encode_type(ast, encoder, inner, type_ids, generics, regions);
        return;
    }

    // A literal stands where a type argument does: `Ffi("libc")` is a
    // different type from `Ffi("libm")`, and the text is the whole of the
    // difference (§7.4).
    if let TypeExpr::Lit(text) = written {
        encoder.tag(tag::STR).str(text);
        return;
    }

    let (name, args) = (
        written.head().expect("a reference and a literal were handled above"),
        written.args().to_vec(),
    );
    encoder.tag(tag::TYPE_NAME);

    // A generic parameter is positional: `f[T](x: T)` and `f[U](x: U)` differ
    // in no way a caller can see.
    if let Some(index) = generics.iter().position(|g| *g == name) {
        encoder.tag(tag::LOCAL).u32(index as u32);
    } else if let Some(hash) = type_ids.get(&name) {
        encoder.tag(tag::FREE).hash(*hash);
    } else {
        encoder.tag(tag::NONE).str(ast.name_of(name));
    }

    encoder.len(args.len());
    for arg in &args {
        encode_type(ast, encoder, *arg, type_ids, generics, regions);
    }
}

fn hash_signature(ast: &Ast, decl: &FnDecl, type_ids: &HashMap<Symbol, Hash>) -> Hash {
    let mut encoder = Encoder::default();
    encoder.str(ast.name_of(decl.name));
    // The *count* of generics, not their names.
    encoder.len(decl.generics.len());
    // The *count* of region parameters, and the `where` clauses as pairs of
    // positions: both are part of the contract, and neither depends on the
    // names chosen.
    encoder.len(decl.regions.len());
    encoder.len(decl.outlives.len());
    for (inner, outer) in &decl.outlives {
        // `u32::MAX` stands for a name that is not a region parameter. The
        // checker refuses that; the hasher runs whether or not it did, and a
        // total encoding is what keeps `lex-sys ids` from panicking on a
        // program that is about to be rejected anyway.
        let position = |sym: &Symbol| {
            decl.regions.iter().position(|r| r == sym).map_or(u32::MAX, |i| i as u32)
        };
        encoder.u32(position(inner));
        encoder.u32(position(outer));
    }
    // The row is what a caller depends on as much as the types are: `[]`
    // means the call is pure and `[io]` means it is not, and a caller's own
    // row has to contain it. Sorted here rather than trusted, so two
    // spellings of one set are one signature.
    encoder.tag(tag::EFFECTS);
    encode_effects(ast, &mut encoder, &decl.effects);
    encoder.len(decl.params.len());
    for param in &decl.params {
        // Parameter names are excluded: lex-sys has no named arguments, so a
        // caller cannot observe them.
        encode_type(ast, &mut encoder, param.ty, type_ids, &decl.generics, &decl.regions);
    }
    encode_type(ast, &mut encoder, decl.ret, type_ids, &decl.generics, &decl.regions);
    encoder.finish(DOMAIN_SIG)
}

/// The row, canonicalised: sorted by (name, argument) and deduplicated, so
/// two spellings of one set are one signature (§7.1).
fn encode_effects(ast: &Ast, encoder: &mut Encoder, effects: &[EffectLabel]) {
    encoder.tag(tag::EFFECTS);
    let mut labels: Vec<(&str, Option<&str>)> =
        effects.iter().map(|e| (ast.name_of(e.name), e.argument.as_deref())).collect();
    labels.sort_unstable();
    labels.dedup();
    encoder.len(labels.len());
    for (name, argument) in labels {
        encoder.str(name);
        match argument {
            Some(text) => {
                encoder.tag(tag::SOME).str(text);
            }
            None => {
                encoder.tag(tag::NONE);
            }
        }
    }
}

/// A foreign signature (§8.4).
///
/// The linker symbol is part of it: two declarations that agree on
/// everything but which C function they bind are not the same declaration.
fn hash_extern(ast: &Ast, decl: &ExternDecl, type_ids: &HashMap<Symbol, Hash>) -> Hash {
    let mut encoder = Encoder::default();
    encoder.tag(tag::EXTERN_DECL);
    encoder.str(ast.name_of(decl.name));
    encoder.str(&decl.symbol);
    encoder.len(decl.regions.len());
    encode_effects(ast, &mut encoder, &decl.effects);
    encoder.len(decl.params.len());
    for param in &decl.params {
        encode_type(ast, &mut encoder, param.ty, type_ids, &[], &decl.regions);
    }
    encode_type(ast, &mut encoder, decl.ret, type_ids, &[], &decl.regions);
    encoder.finish(DOMAIN_SIG)
}

fn hash_struct(ast: &Ast, decl: &StructDecl) -> Hash {
    let mut encoder = Encoder::default();
    encoder.tag(tag::STRUCT_DECL);
    encoder.tag(tag::mode_tag(decl.mode));
    encoder.str(ast.name_of(decl.name));
    encoder.len(decl.generics.len());
    encoder.len(decl.fields.len());
    for field in &decl.fields {
        // Field names *are* observable: a literal names them, and field order
        // is positional to the backend.
        encoder.str(ast.name_of(field.name));
        encode_type(ast, &mut encoder, field.ty, &HashMap::new(), &decl.generics, &[]);
    }
    encoder.finish(DOMAIN_TYPE)
}

fn hash_enum(ast: &Ast, decl: &EnumDecl) -> Hash {
    let mut encoder = Encoder::default();
    encoder.tag(tag::ENUM_DECL);
    encoder.tag(tag::mode_tag(decl.mode));
    encoder.str(ast.name_of(decl.name));
    encoder.len(decl.generics.len());
    encoder.len(decl.variants.len());
    for variant in &decl.variants {
        encoder.str(ast.name_of(variant.name));
        encoder.len(variant.payload.len());
        for ty in &variant.payload {
            encode_type(ast, &mut encoder, *ty, &HashMap::new(), &decl.generics, &[]);
        }
    }
    encoder.finish(DOMAIN_TYPE)
}

/// Tracks which names are bound, so a reference can be encoded by its binder's
/// position rather than its text.
struct Scope {
    /// Binders in the order they were introduced; the last match wins, which
    /// is what shadowing means.
    binders: Vec<Symbol>,
}

impl Scope {
    fn position(&self, name: Symbol) -> Option<usize> {
        self.binders.iter().rposition(|b| *b == name)
    }
}

struct BodyHasher<'a> {
    ast: &'a Ast,
    sig_ids: &'a HashMap<Symbol, Hash>,
    type_ids: &'a HashMap<Symbol, Hash>,
    generics: Vec<Symbol>,
    /// The regions nameable here: the declaration's parameters, plus one per
    /// `borrow` block currently open. Positional, like every other binder.
    regions: Vec<Symbol>,
    scope: Scope,
    encoder: Encoder,
}

fn hash_body(
    ast: &Ast,
    decl: &FnDecl,
    sig_ids: &HashMap<Symbol, Hash>,
    type_ids: &HashMap<Symbol, Hash>,
) -> Hash {
    let mut hasher = BodyHasher {
        ast,
        sig_ids,
        type_ids,
        generics: decl.generics.clone(),
        // Parameters are the outermost binders, so a body referring to its
        // first parameter says "binder 0" whatever that parameter is called.
        scope: Scope { binders: decl.params.iter().map(|p| p.name).collect() },
        // Region parameters are the outermost regions, exactly as parameters
        // are the outermost binders; a `borrow` block pushes onto this.
        regions: decl.regions.clone(),
        encoder: Encoder::default(),
    };
    hasher.block(&decl.body);
    hasher.encoder.finish(DOMAIN_BODY)
}

impl BodyHasher<'_> {
    fn block(&mut self, block: &Block) {
        let depth = self.scope.binders.len();
        self.encoder.len(block.stmts.len());
        for stmt in &block.stmts {
            self.stmt(*stmt);
        }
        // Bindings introduced inside the block leave with it.
        self.scope.binders.truncate(depth);
    }

    fn stmt(&mut self, id: StmtId) {
        match self.ast.stmt(id) {
            Stmt::Let { name, mutable, ty, value } => {
                self.encoder.tag(tag::LET).bool(*mutable);
                match ty {
                    Some(written) => {
                        self.encoder.tag(tag::SOME);
                        let (written, generics) = (*written, self.generics.clone());
                        let regions = self.regions.clone();
                        encode_type(
                            self.ast,
                            &mut self.encoder,
                            written,
                            self.type_ids,
                            &generics,
                            &regions,
                        );
                    }
                    None => {
                        self.encoder.tag(tag::NONE);
                    }
                }
                // The initialiser is encoded *before* the binding exists, so
                // `let x = x;` reads whatever outer `x` it actually reads.
                self.expr(*value);
                self.scope.binders.push(*name);
            }
            Stmt::Assign { place, value } => {
                self.encoder.tag(tag::ASSIGN);
                // The place is an ordinary expression, so it encodes like
                // one: `x = e` and `r.f = e` differ where they differ and
                // nowhere else.
                self.expr(*place);
                self.expr(*value);
            }
            Stmt::Expr(value) => {
                self.encoder.tag(tag::EXPR_STMT);
                self.expr(*value);
            }
            Stmt::If { cond, then_block, else_block } => {
                self.encoder.tag(tag::IF);
                self.expr(*cond);
                self.block(then_block);
                match else_block {
                    Some(block) => {
                        self.encoder.tag(tag::SOME);
                        self.block(block);
                    }
                    None => {
                        self.encoder.tag(tag::NONE);
                    }
                }
            }
            Stmt::While { cond, body } => {
                self.encoder.tag(tag::WHILE);
                self.expr(*cond);
                self.block(body);
            }
            Stmt::Match { scrutinee, arms } => {
                self.encoder.tag(tag::MATCH);
                self.expr(*scrutinee);
                self.encoder.len(arms.len());
                for arm in arms {
                    let depth = self.scope.binders.len();
                    match &arm.pattern {
                        lex_sys_syntax::ast::Pattern::Wildcard => {
                            self.encoder.tag(tag::PATTERN_WILDCARD);
                        }
                        lex_sys_syntax::ast::Pattern::Variant { enum_name, variant, bindings } => {
                            self.encoder.tag(tag::PATTERN_VARIANT);
                            self.type_reference(*enum_name);
                            self.encoder.str(self.ast.name_of(*variant));
                            self.encoder.len(bindings.len());
                            for binding in bindings {
                                match binding {
                                    Some(name) => {
                                        self.encoder.tag(tag::SOME);
                                        self.scope.binders.push(*name);
                                    }
                                    None => {
                                        self.encoder.tag(tag::NONE);
                                    }
                                }
                            }
                        }
                    }
                    self.block(&arm.body);
                    self.scope.binders.truncate(depth);
                }
            }
            Stmt::Borrow { value, unique, region, body } => {
                self.encoder.tag(tag::BORROW).bool(*unique);
                // The value being borrowed is an ordinary name reference.
                self.name(*value);
                // The region and the reference share one name and both are
                // positional, so `borrow f as &r in` and `borrow f as &q in`
                // are one body.
                self.regions.push(*region);
                self.scope.binders.push(*region);
                self.block(body);
                self.scope.binders.pop();
                self.regions.pop();
            }
            Stmt::Region { region, body } => {
                // Positional like a borrow's, and for the same reason: the
                // name is the author's, and no caller and no reader of the
                // hash can tell `region a` from `region q`.
                self.encoder.tag(tag::REGION);
                self.regions.push(*region);
                self.block(body);
                self.regions.pop();
            }
            Stmt::Destructure { struct_name, fields, value } => {
                self.encoder.tag(tag::DESTRUCTURE);
                self.type_reference(*struct_name);
                // Field *names* are encoded, because which field each binder
                // takes is what the pattern says; the binders themselves are
                // positional from here on, like any other local.
                self.encoder.len(fields.len());
                for field in fields {
                    self.encoder.str(self.ast.name_of(*field));
                }
                // The value is encoded before the binders exist, as for `let`.
                self.expr(*value);
                for field in fields {
                    self.scope.binders.push(*field);
                }
            }
            Stmt::Return(value) => {
                self.encoder.tag(tag::RETURN);
                self.expr(*value);
            }
        }
    }

    /// A name: its binder's position if it is bound, otherwise its identity.
    fn name(&mut self, name: Symbol) {
        match self.scope.position(name) {
            Some(index) => {
                self.encoder.tag(tag::LOCAL).u32(index as u32);
            }
            None => match self.sig_ids.get(&name) {
                // A call's target contributes its *signature*, so a callee's
                // body may be rewritten without touching this hash.
                Some(sig) => {
                    self.encoder.tag(tag::FREE).hash(*sig);
                }
                None => {
                    self.encoder.tag(tag::NONE).str(self.ast.name_of(name));
                }
            },
        }
    }

    /// A type mentioned by name in a body, such as an enum in a pattern.
    fn type_reference(&mut self, name: Symbol) {
        match self.type_ids.get(&name) {
            Some(hash) => {
                self.encoder.tag(tag::FREE).hash(*hash);
            }
            None => {
                self.encoder.tag(tag::NONE).str(self.ast.name_of(name));
            }
        }
    }

    /// Which region a name refers to, positionally where it is one in
    /// scope. The name itself never reaches a hash: `region a` and
    /// `region q` are one body.
    fn region_reference(&mut self, region: Symbol) {
        match self.regions.iter().rposition(|r| *r == region) {
            Some(index) => {
                self.encoder.tag(tag::LOCAL).u32(index as u32);
            }
            None => {
                self.encoder.tag(tag::NONE).str(self.ast.name_of(region));
            }
        }
    }

    fn expr(&mut self, id: ExprId) {
        match self.ast.expr(id) {
            Expr::Int(value) => {
                self.encoder.tag(tag::INT).i64(*value);
            }
            Expr::Bool(value) => {
                self.encoder.tag(tag::BOOL).bool(*value);
            }
            Expr::Str(text) => {
                self.encoder.tag(tag::STR).str(text);
            }
            Expr::Name(name) => {
                self.encoder.tag(tag::LOCAL);
                self.name(*name);
            }
            Expr::Index { base, index } => {
                self.encoder.tag(tag::INDEX);
                self.expr(*base);
                self.expr(*index);
            }
            Expr::AllocSlice { region, count, fill } => {
                self.encoder.tag(tag::ALLOC_SLICE);
                self.region_reference(*region);
                self.expr(*count);
                self.expr(*fill);
            }
            Expr::Alloc { region, value } => {
                // Which arena is part of what this expression *means*, so it
                // reaches the hash -- positionally, since the name does not.
                self.encoder.tag(tag::ALLOC);
                self.region_reference(*region);
                self.expr(*value);
            }
            Expr::StructLit { name, fields } => {
                self.encoder.tag(tag::STRUCT_LIT);
                self.type_reference(*name);
                // Field order as written is *not* canonicalised here: the
                // checker reorders into declaration order, and two literals
                // differing only in the order they list fields are the same
                // value. Sorting by name would make that true of the hash too;
                // it is left alone because the declaration's order is not
                // known here, and `docs/canonical-ast.md` §8 keeps it open --
                // as it does for a destructuring pattern, which has the same
                // gap for the same reason.
                self.encoder.len(fields.len());
                for (field, value) in fields {
                    self.encoder.str(self.ast.name_of(*field));
                    self.expr(*value);
                }
            }
            Expr::Field { base, name } => {
                self.encoder.tag(tag::FIELD);
                self.expr(*base);
                self.encoder.str(self.ast.name_of(*name));
            }
            Expr::Variant { enum_name, variant, args } => {
                self.encoder.tag(tag::VARIANT);
                self.type_reference(*enum_name);
                self.encoder.str(self.ast.name_of(*variant));
                self.encoder.len(args.len());
                for arg in args {
                    self.expr(*arg);
                }
            }
            Expr::Unary { op, operand } => {
                self.encoder.tag(tag::UNARY).tag(unary_tag(*op));
                self.expr(*operand);
            }
            Expr::Binary { op, lhs, rhs } => {
                self.encoder.tag(tag::BINARY).tag(binary_tag(*op));
                self.expr(*lhs);
                self.expr(*rhs);
            }
            Expr::Call { callee, args } => {
                self.encoder.tag(tag::CALL);
                self.name(*callee);
                self.encoder.len(args.len());
                for arg in args {
                    self.expr(*arg);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_sys_syntax::parse;

    fn ids(src: &str) -> Identities {
        identify(&parse(src).expect("should parse"))
    }

    fn sig(src: &str, name: &str) -> Hash {
        ids(src).function(name).expect("a function").sig
    }

    fn body(src: &str, name: &str) -> Hash {
        ids(src).function(name).expect("a function").body
    }

    fn ty(src: &str, name: &str) -> Hash {
        ids(src).type_decl(name).expect("a type declaration").id
    }

    // ---- what must not change a hash -----------------------------------

    #[test]
    fn formatting_and_comments_do_not_reach_the_hash() {
        let a = "fn f(n: int) -> [] int { return n + 1; }";
        let b = "fn   f( n : int )  -> [] int {\n    // add one\n    return n + 1;\n}\n";
        assert_eq!(sig(a, "f"), sig(b, "f"));
        assert_eq!(body(a, "f"), body(b, "f"));
    }

    #[test]
    fn redundant_parentheses_do_not_reach_the_hash() {
        assert_eq!(
            body("fn f() -> [] int { return 1 + 2 * 3; }", "f"),
            body("fn f() -> [] int { return (1 + ((2 * 3))); }", "f")
        );
    }

    #[test]
    fn literal_spelling_does_not_reach_the_hash() {
        assert_eq!(
            body("fn f() -> [] int { return 1000; }", "f"),
            body("fn f() -> [] int { return 1_000; }", "f")
        );
    }

    #[test]
    fn a_unit_does_not_depend_on_its_neighbours() {
        // Same function, different company, different order.
        let alone = "fn f(n: int) -> [] int { return n; }";
        let crowded = "fn before() -> [] int { return 0; } \
                       fn f(n: int) -> [] int { return n; } \
                       struct Unrelated { x: int }";
        assert_eq!(sig(alone, "f"), sig(crowded, "f"));
        assert_eq!(body(alone, "f"), body(crowded, "f"));
    }

    #[test]
    fn interner_indices_do_not_reach_the_hash() {
        // `zzz` is mentioned first in one file and not at all in the other, so
        // every Symbol index shifts. Hashing the index would move `f`.
        let a = "fn zzz() -> [] int { return 0; } fn f(n: int) -> [] int { return n; }";
        let b = "fn f(n: int) -> [] int { return n; }";
        assert_eq!(sig(a, "f"), sig(b, "f"));
        assert_eq!(body(a, "f"), body(b, "f"));
    }

    #[test]
    fn renaming_a_local_does_not_change_the_body() {
        assert_eq!(
            body("fn f(n: int) -> [] int { let doubled = n * 2; return doubled; }", "f"),
            body("fn f(n: int) -> [] int { let d = n * 2; return d; }", "f")
        );
    }

    #[test]
    fn renaming_a_parameter_changes_neither_identity() {
        let a = "fn f(value: int) -> [] int { return value + 1; }";
        let b = "fn f(other: int) -> [] int { return other + 1; }";
        assert_eq!(sig(a, "f"), sig(b, "f"));
        assert_eq!(body(a, "f"), body(b, "f"));
    }

    #[test]
    fn renaming_a_type_parameter_changes_nothing() {
        assert_eq!(
            sig("fn f[T](x: T) -> [] T { return x; }", "f"),
            sig("fn f[U](x: U) -> [] U { return x; }", "f")
        );
        assert_eq!(
            body("fn f[T](x: T) -> [] T { return x; }", "f"),
            body("fn f[U](x: U) -> [] U { return x; }", "f")
        );
    }

    #[test]
    fn a_pattern_binding_is_positional_too() {
        let e = "enum E { A(int) } ";
        assert_eq!(
            body(
                &format!("{e}fn f(v: E) -> [] int {{ match v {{ E::A(x) => {{ return x; }} }} }}"),
                "f"
            ),
            body(
                &format!("{e}fn f(v: E) -> [] int {{ match v {{ E::A(y) => {{ return y; }} }} }}"),
                "f"
            )
        );
    }

    // ---- what must change a hash ---------------------------------------

    #[test]
    fn a_different_operator_is_a_different_body() {
        assert_ne!(
            body("fn f(a: int, b: int) -> [] int { return a + b; }", "f"),
            body("fn f(a: int, b: int) -> [] int { return a - b; }", "f")
        );
    }

    #[test]
    fn swapping_two_parameters_changes_the_signature() {
        assert_ne!(
            sig("fn f(a: int, b: bool) -> [] int { return a; }", "f"),
            sig("fn f(a: bool, b: int) -> [] int { return 0; }", "f")
        );
    }

    #[test]
    fn a_shadowed_binding_resolves_to_the_inner_one() {
        // Both read the *inner* `x`, so they agree...
        let inner_a = "fn f() -> [] int { let x = 1; if true { let x = 2; return x; } return 0; }";
        let inner_b = "fn f() -> [] int { let y = 1; if true { let x = 2; return x; } return 0; }";
        assert_eq!(body(inner_a, "f"), body(inner_b, "f"));

        // ...and reading the outer one instead is a different program.
        let outer = "fn f() -> [] int { let x = 1; if true { let y = 2; return x; } return 0; }";
        assert_ne!(body(inner_a, "f"), body(outer, "f"));
    }

    #[test]
    fn field_and_variant_order_are_observable() {
        assert_ne!(
            ids("struct P { x: int, y: bool }").type_decl("P").unwrap().id,
            ids("struct P { y: bool, x: int }").type_decl("P").unwrap().id
        );
        assert_ne!(
            ids("enum E { A, B }").type_decl("E").unwrap().id,
            ids("enum E { B, A }").type_decl("E").unwrap().id
        );
    }

    // ---- the signature / body split ------------------------------------

    #[test]
    fn rewriting_a_body_leaves_its_signature_alone() {
        let slow = "fn double(n: int) -> [] int { return n + n; }";
        let fast = "fn double(n: int) -> [] int { return n * 2; }";
        assert_eq!(sig(slow, "double"), sig(fast, "double"));
        assert_ne!(body(slow, "double"), body(fast, "double"));
    }

    #[test]
    fn a_callers_body_survives_a_callees_rewrite() {
        // The whole point of the split: only a change a caller could observe
        // propagates to the caller.
        let caller = "fn use_it() -> [] int { return double(21); } ";
        let slow = format!("{caller}fn double(n: int) -> [] int {{ return n + n; }}");
        let fast = format!("{caller}fn double(n: int) -> [] int {{ return n * 2; }}");
        assert_eq!(body(&slow, "use_it"), body(&fast, "use_it"));
        assert_ne!(body(&slow, "double"), body(&fast, "double"));
    }

    #[test]
    fn a_callers_body_changes_when_the_callee_signature_does() {
        let caller = "fn use_it() -> [] int { return f(1); } ";
        let a = format!("{caller}fn f(n: int) -> [] int {{ return n; }}");
        let b =
            format!("{caller}fn f(n: int) -> [] int {{ return n; }} struct Unused {{ x: int }}");
        assert_eq!(body(&a, "use_it"), body(&b, "use_it"));

        // Changing the callee's *type* is observable to the caller.
        let changed = format!("{caller}fn f(n: bool) -> [] int {{ return 0; }}");
        assert_ne!(body(&a, "use_it"), body(&changed, "use_it"));
    }

    #[test]
    fn mutual_recursion_terminates() {
        // Neither signature depends on a body, so the graph is acyclic even
        // though the source is not.
        let src = "fn even(n: int) -> [] bool { if n == 0 { return true; } return odd(n - 1); } \
                   fn odd(n: int) -> [] bool { if n == 0 { return false; } return even(n - 1); }";
        let identities = ids(src);
        assert_ne!(
            identities.function("even").unwrap().body,
            identities.function("odd").unwrap().body
        );
    }

    #[test]
    fn a_signature_follows_the_types_it_mentions() {
        let a = "struct P { x: int } fn f(p: P) -> [] int { return p.x; }";
        let b = "struct P { x: bool } fn f(p: P) -> [] int { return 0; }";
        assert_ne!(sig(a, "f"), sig(b, "f"));
    }

    // ---- domains and rendering -----------------------------------------

    #[test]
    fn a_redundant_val_does_not_change_a_type_hash() {
        // `val` on a type whose members are all `val` asserts exactly what
        // absence already checks, and a `val` that was not true is refused
        // before it ever reaches here -- so it is a redundant annotation, in
        // the same family as a redundant parenthesis.
        assert_eq!(
            ty("struct P { x: int, y: int }", "P"),
            ty("val struct P { x: int, y: int }", "P")
        );
        assert_eq!(ty("enum E { A, B(int) }", "E"), ty("val enum E { A, B(int) }", "E"));
    }

    #[test]
    fn res_changes_a_type_hash() {
        // A `res` type is not the same type as a `val` one: one is linear and
        // one is copyable, and every caller can tell.
        assert_ne!(ty("struct F { fd: int }", "F"), ty("res struct F { fd: int }", "F"));
        assert_ne!(ty("enum H { A }", "H"), ty("res enum H { A }", "H"));
    }

    #[test]
    fn destructuring_is_its_own_statement() {
        assert_ne!(
            body("struct P { x: int } fn f(p: P) -> [] int { let P { x } = p; return x; }", "f"),
            body("struct P { x: int } fn f(p: P) -> [] int { let x = p.x; return x; }", "f")
        );
    }

    #[test]
    fn a_destructuring_pattern_hashes_the_field_order_it_wrote() {
        // A pattern binds by field name, so the two bodies below mean
        // exactly the same thing and hash differently anyway. That is the
        // same gap a struct literal's field order has, for the same reason
        // -- the declaration's order is not known here -- and
        // `docs/canonical-ast.md` §8 records both as open rather than
        // pretending otherwise.
        let written = "struct P { x: int, y: int } \
                 fn f(p: P) -> [] int { let P { x, y } = p; return x; }";
        let reordered = "struct P { x: int, y: int } \
                 fn f(p: P) -> [] int { let P { y, x } = p; return x; }";
        assert_ne!(body(written, "f"), body(reordered, "f"));
    }

    // ---- regions (`docs/linearity-and-effects.md` §5) -------------------

    #[test]
    fn a_region_parameter_is_positional() {
        // The same argument as a type parameter: no caller can tell `r` from
        // `q`, so the two signatures are one.
        assert_eq!(
            sig("fn len[&r](s: &r int) -> [] int { return 0; }", "len"),
            sig("fn len[&q](s: &q int) -> [] int { return 0; }", "len")
        );
    }

    #[test]
    fn a_reference_is_not_its_referent() {
        assert_ne!(
            sig("fn f(s: &r int) -> [] int { return 0; }", "f"),
            sig("fn f(s: int) -> [] int { return 0; }", "f")
        );
    }

    #[test]
    fn uniqueness_is_part_of_a_signature() {
        assert_ne!(
            sig("fn f[&r](s: &r int) -> [] int { return 0; }", "f"),
            sig("fn f[&r](s: &!r int) -> [] int { return 0; }", "f")
        );
    }

    #[test]
    fn which_region_a_parameter_names_is_part_of_a_signature() {
        assert_ne!(
            sig("fn f[&a, &b](x: &a int, y: &b int) -> [] int { return 0; }", "f"),
            sig("fn f[&a, &b](x: &a int, y: &a int) -> [] int { return 0; }", "f")
        );
    }

    #[test]
    fn a_where_clause_is_part_of_a_signature() {
        // It is an obligation on every caller, so it is exactly the kind of
        // thing `SigId` exists to cover.
        assert_ne!(
            sig("fn f[&a, &b](x: &a int, y: &b int) -> [] int { return 0; }", "f"),
            sig("fn f[&a, &b where b <= a](x: &a int, y: &b int) -> [] int { return 0; }", "f")
        );
    }

    #[test]
    fn a_borrow_blocks_region_name_does_not_reach_the_body_hash() {
        // Positional inside a body too, like any other binder.
        assert_eq!(
            body("fn f(x: int) -> [] int { borrow x as &r in { return 0; } return 1; }", "f"),
            body("fn f(x: int) -> [] int { borrow x as &q in { return 0; } return 1; }", "f")
        );
    }

    #[test]
    fn what_is_borrowed_does_reach_the_body_hash() {
        assert_ne!(
            body(
                "fn f(x: int, y: int) -> [] int { borrow x as &r in { return 0; } return 1; }",
                "f"
            ),
            body(
                "fn f(x: int, y: int) -> [] int { borrow y as &r in { return 0; } return 1; }",
                "f"
            )
        );
    }

    #[test]
    fn a_shared_borrow_is_not_a_unique_one() {
        assert_ne!(
            body("fn f(x: int) -> [] int { borrow x as &r in { return 0; } return 1; }", "f"),
            body("fn f(x: int) -> [] int { borrow mut x as &!r in { return 0; } return 1; }", "f")
        );
    }

    #[test]
    fn an_assignments_place_reaches_the_body_hash() {
        assert_ne!(
            body("struct P { x: int, y: int } fn f(r: P) -> [] int { r.x = 1; return 0; }", "f"),
            body("struct P { x: int, y: int } fn f(r: P) -> [] int { r.y = 1; return 0; }", "f")
        );
    }

    #[test]
    fn writing_through_a_reference_is_not_writing_a_local() {
        assert_ne!(
            body("struct P { x: int } fn f(r: P) -> [] int { r.x = 1; return 0; }", "f"),
            body("struct P { x: int } fn f(r: P) -> [] int { r = P { x: 1 }; return 0; }", "f")
        );
    }

    // ---- effect rows (`docs/linearity-and-effects.md` §7) ---------------

    #[test]
    fn a_row_is_part_of_a_signature() {
        // A caller depends on it as much as on the types: `[]` means the call
        // is pure and `[io]` means the caller's own row has to contain it.
        assert_ne!(
            sig("fn f() -> [] int { return 0; }", "f"),
            sig("fn f() -> [io] int { return putchar(0); }", "f")
        );
    }

    #[test]
    fn a_rows_written_order_does_not_reach_the_hash() {
        // §7.1: a *set*, canonically ordered. Two spellings of one set are
        // one signature, which is what makes the row hashable at all.
        assert_eq!(
            sig("fn f() -> [fs, io] int { return 0; }", "f"),
            sig("fn f() -> [io, fs] int { return 0; }", "f")
        );
    }

    #[test]
    fn a_duplicate_label_does_not_reach_the_hash() {
        assert_eq!(
            sig("fn f() -> [io] int { return 0; }", "f"),
            sig("fn f() -> [io, io] int { return 0; }", "f")
        );
    }

    #[test]
    fn a_wider_row_is_a_different_signature() {
        assert_ne!(
            sig("fn f() -> [io] int { return 0; }", "f"),
            sig("fn f() -> [fs, io] int { return 0; }", "f")
        );
    }

    #[test]
    fn a_row_does_not_reach_the_body_hash() {
        // The row is a contract, so it lives in `SigId`. A body that did not
        // change has not changed.
        assert_eq!(
            body("fn f() -> [] int { return 1 + 1; }", "f"),
            body("fn f() -> [io] int { return 1 + 1; }", "f")
        );
    }

    // ---- arenas (§6) ---------------------------------------------------

    #[test]
    fn an_arenas_name_does_not_reach_the_hash() {
        // Positional like a borrow's region, and for the same reason: no
        // caller and no reader can tell `region a` from `region q`.
        assert_eq!(
            body(
                "struct N { v: int } fn f() -> [] int { region a { let n = alloc[a](N { v: 1 }); } return 0; }",
                "f"
            ),
            body(
                "struct N { v: int } fn f() -> [] int { region q { let n = alloc[q](N { v: 1 }); } return 0; }",
                "f"
            )
        );
    }

    #[test]
    fn which_arena_an_allocation_goes_in_reaches_the_hash() {
        // Two nested arenas, and an allocation that moves from the inner to
        // the outer is a different body: where a value lives is what the
        // expression means, not decoration.
        assert_ne!(
            body(
                "struct N { v: int } fn f() -> [] int { region o { region i { let n = alloc[i](N { v: 1 }); } } return 0; }",
                "f"
            ),
            body(
                "struct N { v: int } fn f() -> [] int { region o { region i { let n = alloc[o](N { v: 1 }); } } return 0; }",
                "f"
            )
        );
    }

    #[test]
    fn an_arena_is_not_a_borrow_block() {
        // Both open a region and both are a block; they are still two
        // different statements, and a hash that confused them would say a
        // rewritten body had not changed.
        assert_ne!(
            body(
                "struct N { v: int } fn f() -> [] int { let b = N { v: 1 }; region a { let x = 1; } return 0; }",
                "f"
            ),
            body(
                "struct N { v: int } fn f() -> [] int { let b = N { v: 1 }; borrow b as &a in { let x = 1; } return 0; }",
                "f"
            )
        );
    }

    // ---- slices --------------------------------------------------------

    #[test]
    fn a_slice_is_a_different_type_from_its_element() {
        assert_ne!(
            sig("fn f[&r](xs: &r [int]) -> [] int { return 0; }", "f"),
            sig("fn f[&r](xs: &r int) -> [] int { return 0; }", "f")
        );
        assert_ne!(
            sig("fn f[&r](xs: &r [int]) -> [] int { return 0; }", "f"),
            sig("fn f[&r](xs: &r [bool]) -> [] int { return 0; }", "f")
        );
    }

    #[test]
    fn an_index_reaches_the_body_hash() {
        // Which element is read is what the expression *means*, so two
        // bodies reading different ones are two bodies.
        assert_ne!(
            body("fn f[&r](xs: &r [int]) -> [] int { return xs[0]; }", "f"),
            body("fn f[&r](xs: &r [int]) -> [] int { return xs[1]; }", "f")
        );
        // And indexing is not field access, whatever the offsets work out to.
        assert_ne!(
            body("fn f[&r](xs: &r [int]) -> [] int { return xs[0]; }", "f"),
            body("fn f[&r](xs: &r [int]) -> [] int { return len(xs); }", "f")
        );
    }

    #[test]
    fn a_slices_arena_reaches_the_body_hash_and_its_name_does_not() {
        assert_eq!(
            body("fn f() -> [] int { region a { let xs = alloc_slice[a](1, 0); } return 0; }", "f"),
            body("fn f() -> [] int { region q { let xs = alloc_slice[q](1, 0); } return 0; }", "f")
        );
        assert_ne!(
            body(
                "fn f() -> [] int { region o { region i { let xs = alloc_slice[i](1, 0); } } return 0; }",
                "f"
            ),
            body(
                "fn f() -> [] int { region o { region i { let xs = alloc_slice[o](1, 0); } } return 0; }",
                "f"
            )
        );
    }

    // ---- narrowing and foreign declarations (§7.4, §8.4) ---------------

    #[test]
    fn a_labels_argument_is_part_of_the_signature() {
        // §7.4: `ffi` and `ffi("libc")` are different labels, and a caller
        // depends on which one it is exactly as it depends on the types.
        assert_ne!(
            sig("fn f() -> [ffi] int { return 0; }", "f"),
            sig("fn f() -> [ffi(\"libc\")] int { return 0; }", "f")
        );
        assert_ne!(
            sig("fn f() -> [ffi(\"libc\")] int { return 0; }", "f"),
            sig("fn f() -> [ffi(\"libm\")] int { return 0; }", "f")
        );
    }

    #[test]
    fn a_foreign_declaration_has_an_identity_like_any_other() {
        // It is a signature with no body, so `SigId` and `BodyId` are the
        // same hash: there is nothing else it could be a hash of.
        let identities =
            ids("extern fn labs[&f](ffi: &f Ffi(\"libc\"), n: int) -> [ffi(\"libc\")] int;");
        let labs = identities.function("labs").expect("the foreign declaration");
        assert_eq!(labs.sig, labs.body);
    }

    #[test]
    fn the_library_a_foreign_function_names_reaches_its_hash() {
        // Two declarations differing only in which library they reach are
        // two different contracts, and a caller is entitled to notice.
        assert_ne!(
            sig("extern fn f[&c](ffi: &c Ffi(\"libc\")) -> [ffi(\"libc\")] int;", "f"),
            sig("extern fn f[&c](ffi: &c Ffi(\"libm\")) -> [ffi(\"libm\")] int;", "f")
        );
    }

    #[test]
    fn the_domains_keep_the_three_kinds_apart() {
        let identities = ids("fn f() -> [] int { return 0; } struct S { x: int }");
        let f = identities.function("f").unwrap();
        assert_ne!(f.sig, f.body);
        assert_ne!(f.sig, identities.type_decl("S").unwrap().id);
    }

    #[test]
    fn hashes_render_as_hex() {
        let hash = sig("fn f() -> [] int { return 0; }", "f");
        assert_eq!(hash.to_hex().len(), 64);
        assert_eq!(hash.short().len(), 16);
        assert!(hash.to_hex().starts_with(&hash.short()));
        assert!(hash.to_hex().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn hashing_is_a_function_of_the_program_alone() {
        // Twice over the same text, two parses, same bytes out.
        let src = "fn f(n: int) -> [] int { return n * n; }";
        assert_eq!(body(src, "f"), body(src, "f"));
    }
}
