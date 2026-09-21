//! The type vocabulary: what a type *is*, and how two of them are made equal.
//!
//! This crate holds no opinion about programs. It knows types, substitution and
//! unification; `lex-sys-ir` drives it while resolving and lowering, which is
//! the one pass that reads a function body.
//!
//! Two commitments from `docs/linearity-and-effects.md` §13 shape it, because
//! M2 cannot be bolted onto a checker that got them wrong:
//!
//! * **A type is a value that compares cheaply and could be hashed
//!   canonically.** Structural equality on `Type` is the real thing, argument
//!   lists are `Vec`s in declaration order, and nothing about a type depends on
//!   where it was written.
//! * **Inference is local.** Variables exist so a `let` without an annotation
//!   and a generic call can be solved within one body. A signature is never
//!   inferred — every parameter and return type is written — so unification
//!   never has to cross a function boundary.

use std::fmt;

/// An inference variable, solved within a single function body and never
/// escaping it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct TyVar(pub u32);

/// A declared type's identity: an index into whatever table the driver keeps.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct DefId(pub u32);

/// An unsolved region, instantiated at a call site.
///
/// §5.1: "first-order unification of a single name — not constraint solving,
/// and not something that can fail to terminate." A region variable is solved
/// by one assignment, exactly like a `TyVar`, and the two are kept apart
/// because a region is not a type and must never unify with one.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct RegionVar(pub u32);

/// Where a reference is valid (`docs/linearity-and-effects.md` §5).
///
/// A region is a *block*, full stop — there are no non-lexical lifetimes and
/// nothing is inferred about extent. That is what lets §5.2 call the outlives
/// relation a stack: `Block(i)` is the `i`th `borrow` block open around the
/// expression being checked, counting outwards from zero, so `a <= b` is a
/// comparison of two integers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Region {
    /// A region parameter of the enclosing declaration — the `i`th written in
    /// `[&a, &b]`. Rigid inside the body, and outlives every block in it,
    /// because it was already open when the caller called.
    Param(u32),
    /// A region introduced by a `borrow` block, numbered by nesting depth.
    Block(u32),
    /// The region a string literal's bytes live in (`docs/strings.md` §4).
    ///
    /// It outlives everything and nothing outlives it, because the data is
    /// in the object file rather than in any frame. It is not writable in
    /// source: the only way to get a reference into it is to write a
    /// literal, so there is nothing to name.
    Static,
    /// Unsolved, pending a call site.
    Var(RegionVar),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Type {
    Int,
    /// An 8-bit unsigned integer — *storage*, not arithmetic
    /// (`docs/strings.md` §2).
    ///
    /// There is no `+` on it. `byte_of` and `int_of` convert, which is what
    /// keeps `defined-behaviour.md` §8's deferral of unsigned arithmetic
    /// intact: a type you cannot add to never asks whether adding traps.
    Byte,
    Bool,
    /// The type of an expression that yields nothing, such as a call used as a
    /// statement. Not writable in source.
    Unit,
    /// A rigid generic parameter — `T` inside the body that declares it. It
    /// unifies with itself and nothing else, which is what makes a generic
    /// function checked once rather than once per instantiation.
    Param(u32),
    /// A declared aggregate applied to its arguments.
    Named(DefId, Vec<Type>),
    /// `&r T` and `&!r T`: a non-owning reference, valid for `region`.
    ///
    /// Mode `val` (§5 rule 3) — copyable and discardable, which is sound
    /// precisely because the referent is frozen or locked for the whole
    /// region and the region is a block.
    Ref {
        unique: bool,
        region: Region,
        inner: Box<Type>,
    },
    /// `[T]` — a run of `T`s of a length known only at runtime.
    ///
    /// Unsized, like a struct is sized: it is a *referent* shape, never a
    /// value on its own. `&!a [int]` is the slice, and it is an ordinary
    /// reference, which is the point — every rule §5 gave references
    /// (regions, outlives, the escape check, the unique-to-shared
    /// coercion) applies to a slice without a second mechanism.
    Slice(Box<Type>),
    /// `(A, B)` — an anonymous aggregate with positional components
    /// (`docs/tuples.md`).
    ///
    /// The one **structural** type here: every other aggregate is a
    /// `DefId` into a table of declarations, so two declarations with
    /// identical fields are two types, while `(int, bool)` is one type
    /// wherever it is written. That is what lets two files agree on a
    /// type with neither declaring it (§2.2).
    ///
    /// Two components or more, always. There is no one-tuple, because
    /// `(e)` is grouping, and no `()`, because whether a function may
    /// return nothing is a different question (§2.1).
    Tuple(Vec<Type>),
    /// A type indexed by a literal: the `"libc"` in `Ffi("libc")` (§7.4).
    ///
    /// A singleton — it unifies with itself and nothing else — which is what
    /// makes a narrowed capability a different *type* from a wider one
    /// rather than a differently-tagged value.
    Lit(String),
    /// An unsolved inference variable.
    Var(TyVar),
}

impl Type {
    /// Does `var` occur anywhere in this type? The check that keeps
    /// unification from building an infinite type.
    pub fn occurs(&self, var: TyVar) -> bool {
        match self {
            Type::Var(v) => *v == var,
            Type::Named(_, args) | Type::Tuple(args) => args.iter().any(|a| a.occurs(var)),
            Type::Ref { inner, .. } | Type::Slice(inner) => inner.occurs(var),
            _ => false,
        }
    }

    /// Does `region` occur anywhere in this type?
    ///
    /// §5 rule 4: "Escape is an occurs-check. The type of a `borrow` block's
    /// result may not mention `r`." This is that check — one traversal of one
    /// type, which cannot diverge because a type is finite.
    pub fn mentions(&self, region: Region) -> bool {
        match self {
            Type::Named(_, args) | Type::Tuple(args) => args.iter().any(|a| a.mentions(region)),
            Type::Ref { region: r, inner, .. } => *r == region || inner.mentions(region),
            Type::Slice(inner) => inner.mentions(region),
            _ => false,
        }
    }

    /// Every region this type mentions, outermost occurrence first.
    pub fn regions_into(&self, out: &mut Vec<Region>) {
        match self {
            Type::Named(_, args) | Type::Tuple(args) => {
                args.iter().for_each(|a| a.regions_into(out))
            }
            Type::Ref { region, inner, .. } => {
                out.push(*region);
                inner.regions_into(out);
            }
            Type::Slice(inner) => inner.regions_into(out),
            _ => {}
        }
    }

    pub fn is_var(&self) -> bool {
        matches!(self, Type::Var(_))
    }

    /// Does an unsolved variable survive anywhere in this type?
    pub fn has_var(&self) -> bool {
        match self {
            Type::Var(_) => true,
            Type::Named(_, args) | Type::Tuple(args) => args.iter().any(Type::has_var),
            Type::Ref { inner, .. } | Type::Slice(inner) => inner.has_var(),
            _ => false,
        }
    }

    /// Replace each `Param(i)` with `types[i]` and each `Region::Param(i)`
    /// with `regions[i]`.
    ///
    /// Both at once, deliberately: a generic function may be polymorphic in
    /// types *and* regions, and substituting one without the other leaves a
    /// signature half-instantiated. Taking both arguments means the compiler
    /// finds every call site if that ever changes again.
    pub fn substitute(&self, types: &[Type], regions: &[Region]) -> Type {
        match self {
            Type::Param(i) => types
                .get(*i as usize)
                .cloned()
                .unwrap_or_else(|| panic!("no argument for type parameter {i}")),
            Type::Named(def, inner) => {
                Type::Named(*def, inner.iter().map(|t| t.substitute(types, regions)).collect())
            }
            Type::Tuple(parts) => {
                Type::Tuple(parts.iter().map(|t| t.substitute(types, regions)).collect())
            }
            Type::Ref { unique, region, inner } => Type::Ref {
                unique: *unique,
                region: region.substitute(regions),
                inner: Box::new(inner.substitute(types, regions)),
            },
            Type::Slice(inner) => Type::Slice(Box::new(inner.substitute(types, regions))),
            other => other.clone(),
        }
    }
}

impl Region {
    /// Replace `Param(i)` with `regions[i]`, leaving anything else alone.
    pub fn substitute(&self, regions: &[Region]) -> Region {
        match self {
            Region::Param(i) => regions.get(*i as usize).copied().unwrap_or(*self),
            other => *other,
        }
    }

    pub fn is_var(&self) -> bool {
        matches!(self, Region::Var(_))
    }
}

/// Why two types could not be made equal.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum UnifyError {
    /// The two types are different and neither is a variable.
    Mismatch { expected: Type, found: Type },
    /// Solving would build an infinite type, as in `T = Pair[T, T]`.
    Infinite { var: TyVar, ty: Type },
    /// Two references name different regions, and neither is a variable.
    /// §5.2's sibling case: `x` and `y` come from different `borrow` blocks
    /// and neither outlives the other.
    Regions { expected: Region, found: Region },
    /// A shared reference met a unique one, or the other way round.
    Uniqueness { expected: bool },
}

/// A union-find over inference variables.
#[derive(Default, Debug)]
pub struct Unifier {
    /// `None` while unsolved; solving is one assignment and is never undone.
    solved: Vec<Option<Type>>,
    /// The same, for regions. Kept in its own table because a region is not a
    /// type and the two must never be solved to each other.
    solved_regions: Vec<Option<Region>>,
    /// Names for rendering a `Named` type in a diagnostic. The driver supplies
    /// them; this crate never invents one.
    names: Vec<String>,
    /// The type parameters of whichever declaration is being checked, so a
    /// diagnostic can say `T` rather than `T0`.
    param_names: Vec<String>,
    /// The same for its region parameters, so a diagnostic says `&r`.
    region_param_names: Vec<String>,
    /// The `borrow` blocks open around the expression being checked,
    /// outermost first, so a diagnostic can name the region a reference came
    /// from rather than printing its depth.
    region_block_names: Vec<String>,
}

impl Unifier {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a declared type's name, returning its id. Ids are handed out in
    /// declaration order, so they are a deterministic function of the source.
    pub fn declare(&mut self, name: impl Into<String>) -> DefId {
        self.names.push(name.into());
        DefId(self.names.len() as u32 - 1)
    }

    pub fn name_of(&self, def: DefId) -> &str {
        &self.names[def.0 as usize]
    }

    /// Set the parameter names used when rendering `Type::Param`, for the
    /// declaration about to be checked.
    pub fn set_param_names(&mut self, names: Vec<String>) {
        self.param_names = names;
    }

    /// Set the names used when rendering `Region::Param`, for the declaration
    /// about to be checked.
    pub fn set_region_param_names(&mut self, names: Vec<String>) {
        self.region_param_names = names;
    }

    /// Set the names of the `borrow` blocks currently open, outermost first.
    /// The driver keeps this in step with its own block stack; this crate
    /// never invents a name.
    pub fn set_region_block_names(&mut self, names: Vec<String>) {
        self.region_block_names = names;
    }

    pub fn fresh(&mut self) -> Type {
        self.solved.push(None);
        Type::Var(TyVar(self.solved.len() as u32 - 1))
    }

    pub fn fresh_region(&mut self) -> Region {
        self.solved_regions.push(None);
        Region::Var(RegionVar(self.solved_regions.len() as u32 - 1))
    }

    /// Follow a solved region to whatever it stands for.
    pub fn resolve_region(&self, region: Region) -> Region {
        let mut current = region;
        while let Region::Var(v) = current {
            match self.solved_regions[v.0 as usize] {
                Some(next) => current = next,
                None => break,
            }
        }
        current
    }

    /// Make two regions the same one, or say they are different.
    ///
    /// Exact: the *outlives* coercion of §5.2 is not unification and does not
    /// live here, because whether one block encloses another is a fact about
    /// the body being checked and this crate holds no opinion about programs.
    pub fn unify_regions(&mut self, expected: Region, found: Region) -> Result<(), UnifyError> {
        let a = self.resolve_region(expected);
        let b = self.resolve_region(found);
        match (a, b) {
            (x, y) if x == y => Ok(()),
            (Region::Var(v), other) | (other, Region::Var(v)) => {
                self.solved_regions[v.0 as usize] = Some(other);
                Ok(())
            }
            (x, y) => Err(UnifyError::Regions { expected: x, found: y }),
        }
    }

    /// Follow solved variables one level at a time until the head is not a
    /// solved variable.
    pub fn shallow(&self, ty: &Type) -> Type {
        let mut current = ty.clone();
        while let Type::Var(v) = current {
            match &self.solved[v.0 as usize] {
                Some(next) => current = next.clone(),
                None => break,
            }
        }
        current
    }

    /// Resolve a type all the way down, for reporting or for lowering. An
    /// unsolved variable survives as itself, and the caller decides whether
    /// that is an error.
    pub fn resolve(&self, ty: &Type) -> Type {
        match self.shallow(ty) {
            Type::Slice(inner) => Type::Slice(Box::new(self.resolve(&inner))),
            Type::Named(def, args) => {
                Type::Named(def, args.iter().map(|a| self.resolve(a)).collect())
            }
            Type::Tuple(parts) => Type::Tuple(parts.iter().map(|p| self.resolve(p)).collect()),
            Type::Ref { unique, region, inner } => Type::Ref {
                unique,
                region: self.resolve_region(region),
                inner: Box::new(self.resolve(&inner)),
            },
            other => other,
        }
    }

    /// Make two types equal, or say why they cannot be.
    pub fn unify(&mut self, expected: &Type, found: &Type) -> Result<(), UnifyError> {
        let a = self.shallow(expected);
        let b = self.shallow(found);
        match (a, b) {
            (Type::Var(x), Type::Var(y)) if x == y => Ok(()),
            (Type::Var(x), other) | (other, Type::Var(x)) => {
                if other.occurs(x) {
                    return Err(UnifyError::Infinite { var: x, ty: self.resolve(&other) });
                }
                self.solved[x.0 as usize] = Some(other);
                Ok(())
            }
            (Type::Named(d1, a1), Type::Named(d2, a2)) if d1 == d2 && a1.len() == a2.len() => {
                for (x, y) in a1.iter().zip(a2.iter()) {
                    self.unify(x, y)?;
                }
                Ok(())
            }
            // A reference's referent is invariant: §5 says "`T` never
            // changes". Only the region has a coercion, and that one is the
            // driver's to apply before it gets here.
            (
                Type::Ref { unique: u1, region: r1, inner: i1 },
                Type::Ref { unique: u2, region: r2, inner: i2 },
            ) => {
                if u1 != u2 {
                    return Err(UnifyError::Uniqueness { expected: u1 });
                }
                self.unify_regions(r1, r2)?;
                self.unify(&i1, &i2)
            }
            // A slice's element type is invariant for the same reason a
            // referent is: `[T]` is what a reference points at, and "`T`
            // never changes" does not stop being true one level down.
            (Type::Slice(a), Type::Slice(b)) => self.unify(&a, &b),
            // `docs/tuples.md` §2.2: a tuple is structural, so two of them
            // are the same type when their components are. Arity is part of
            // that -- `(int, bool)` and `(int, bool, int)` mismatch rather
            // than unifying the prefix.
            (Type::Tuple(a), Type::Tuple(b)) if a.len() == b.len() => {
                for (x, y) in a.iter().zip(b.iter()) {
                    self.unify(x, y)?;
                }
                Ok(())
            }
            (x, y) if x == y => Ok(()),
            (x, y) => {
                Err(UnifyError::Mismatch { expected: self.resolve(&x), found: self.resolve(&y) })
            }
        }
    }

    /// Render a type the way it would be written in source.
    pub fn display(&self, ty: &Type) -> String {
        match self.resolve(ty) {
            Type::Int => "int".to_owned(),
            Type::Byte => "byte".to_owned(),
            Type::Bool => "bool".to_owned(),
            Type::Unit => "()".to_owned(),
            Type::Param(i) => {
                self.param_names.get(i as usize).cloned().unwrap_or_else(|| format!("T{i}"))
            }
            Type::Var(v) => format!("?{}", v.0),
            Type::Lit(text) => format!("\"{text}\""),
            Type::Slice(inner) => format!("[{}]", self.display(&inner)),
            Type::Tuple(parts) => {
                let inner: Vec<String> = parts.iter().map(|p| self.display(p)).collect();
                format!("({})", inner.join(", "))
            }
            Type::Named(def, args) if args.is_empty() => self.name_of(def).to_owned(),
            // A literal argument is written the way the source writes it:
            // `Ffi("libc")`, not `Ffi["libc"]`. What a capability is
            // narrowed to travels in its type, but it is not a type.
            Type::Named(def, args) if args.iter().all(|a| matches!(a, Type::Lit(_))) => {
                let inner: Vec<String> = args.iter().map(|a| self.display(a)).collect();
                format!("{}({})", self.name_of(def), inner.join(", "))
            }
            Type::Named(def, args) => {
                let inner: Vec<String> = args.iter().map(|a| self.display(a)).collect();
                format!("{}[{}]", self.name_of(def), inner.join(", "))
            }
            Type::Ref { unique, region, inner } => {
                let bang = if unique { "!" } else { "" };
                format!("&{}{} {}", bang, self.display_region(region), self.display(&inner))
            }
        }
    }

    /// Render a region the way it was written.
    pub fn display_region(&self, region: Region) -> String {
        match self.resolve_region(region) {
            Region::Param(i) => {
                self.region_param_names.get(i as usize).cloned().unwrap_or_else(|| format!("r{i}"))
            }
            Region::Block(i) => {
                self.region_block_names.get(i as usize).cloned().unwrap_or_else(|| format!("r{i}"))
            }
            Region::Static => "static".to_owned(),
            Region::Var(v) => format!("?r{}", v.0),
        }
    }
}

impl fmt::Display for TyVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "?{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concrete_types_unify_only_with_themselves() {
        let mut u = Unifier::new();
        assert!(u.unify(&Type::Int, &Type::Int).is_ok());
        let err = u.unify(&Type::Int, &Type::Bool).unwrap_err();
        assert_eq!(err, UnifyError::Mismatch { expected: Type::Int, found: Type::Bool });
    }

    #[test]
    fn a_variable_solves_to_whatever_it_meets() {
        let mut u = Unifier::new();
        let v = u.fresh();
        u.unify(&v, &Type::Int).unwrap();
        assert_eq!(u.resolve(&v), Type::Int);
        // and stays solved
        assert!(u.unify(&v, &Type::Bool).is_err());
    }

    #[test]
    fn solving_is_transitive() {
        let mut u = Unifier::new();
        let a = u.fresh();
        let b = u.fresh();
        u.unify(&a, &b).unwrap();
        u.unify(&b, &Type::Bool).unwrap();
        assert_eq!(u.resolve(&a), Type::Bool);
    }

    #[test]
    fn arguments_unify_pairwise() {
        let mut u = Unifier::new();
        let pair = u.declare("Pair");
        let v = u.fresh();
        u.unify(
            &Type::Named(pair, vec![Type::Int, v.clone()]),
            &Type::Named(pair, vec![Type::Int, Type::Bool]),
        )
        .unwrap();
        assert_eq!(u.resolve(&v), Type::Bool);
    }

    #[test]
    fn different_constructors_do_not_unify() {
        let mut u = Unifier::new();
        let a = u.declare("A");
        let b = u.declare("B");
        assert!(u.unify(&Type::Named(a, vec![]), &Type::Named(b, vec![])).is_err());
    }

    #[test]
    fn an_infinite_type_is_refused_rather_than_built() {
        let mut u = Unifier::new();
        let list = u.declare("List");
        let v = u.fresh();
        let Type::Var(var) = v else { panic!() };
        let err = u.unify(&v, &Type::Named(list, vec![v.clone()])).unwrap_err();
        assert!(matches!(err, UnifyError::Infinite { var: got, .. } if got == var));
    }

    #[test]
    fn rigid_parameters_unify_only_with_themselves() {
        let mut u = Unifier::new();
        assert!(u.unify(&Type::Param(0), &Type::Param(0)).is_ok());
        assert!(u.unify(&Type::Param(0), &Type::Param(1)).is_err());
        assert!(u.unify(&Type::Param(0), &Type::Int).is_err());
    }

    #[test]
    fn substitution_replaces_parameters_everywhere() {
        let mut u = Unifier::new();
        let pair = u.declare("Pair");
        let generic = Type::Named(
            pair,
            vec![Type::Param(0), Type::Named(pair, vec![Type::Param(1), Type::Param(0)])],
        );
        let applied = generic.substitute(&[Type::Int, Type::Bool], &[]);
        assert_eq!(
            applied,
            Type::Named(pair, vec![Type::Int, Type::Named(pair, vec![Type::Bool, Type::Int])])
        );
    }

    #[test]
    fn display_reads_like_source() {
        let mut u = Unifier::new();
        let pair = u.declare("Pair");
        assert_eq!(u.display(&Type::Int), "int");
        assert_eq!(u.display(&Type::Named(pair, vec![Type::Int, Type::Bool])), "Pair[int, bool]");
        let v = u.fresh();
        assert_eq!(u.display(&v), "?0");
    }

    // ---- regions and references (`docs/linearity-and-effects.md` §5) -----

    #[test]
    fn a_reference_unifies_with_itself() {
        let mut u = Unifier::new();
        let a = Type::Ref { unique: false, region: Region::Block(0), inner: Box::new(Type::Int) };
        assert!(u.unify(&a, &a.clone()).is_ok());
    }

    #[test]
    fn a_reference_does_not_unify_with_its_referent() {
        // `&r int` is not `int`. Without this a borrow would be a no-op.
        let mut u = Unifier::new();
        let r = Type::Ref { unique: false, region: Region::Block(0), inner: Box::new(Type::Int) };
        assert!(u.unify(&r, &Type::Int).is_err());
    }

    #[test]
    fn shared_and_unique_references_are_different_types() {
        let mut u = Unifier::new();
        let shared =
            Type::Ref { unique: false, region: Region::Block(0), inner: Box::new(Type::Int) };
        let unique =
            Type::Ref { unique: true, region: Region::Block(0), inner: Box::new(Type::Int) };
        assert_eq!(u.unify(&shared, &unique), Err(UnifyError::Uniqueness { expected: false }));
    }

    #[test]
    fn two_blocks_regions_do_not_unify() {
        // §5.2's sibling case. Unification is exact here; the *outlives*
        // coercion is the driver's, because it depends on the body.
        let mut u = Unifier::new();
        let x = Type::Ref { unique: false, region: Region::Block(0), inner: Box::new(Type::Int) };
        let y = Type::Ref { unique: false, region: Region::Block(1), inner: Box::new(Type::Int) };
        assert_eq!(
            u.unify(&x, &y),
            Err(UnifyError::Regions { expected: Region::Block(0), found: Region::Block(1) })
        );
    }

    #[test]
    fn a_region_variable_solves_to_whatever_it_meets() {
        // §5.1: instantiating a function's region parameter at a call site is
        // one assignment, not constraint solving.
        let mut u = Unifier::new();
        let v = u.fresh_region();
        assert!(u.unify_regions(v, Region::Block(2)).is_ok());
        assert_eq!(u.resolve_region(v), Region::Block(2));
        assert!(u.unify_regions(v, Region::Block(3)).is_err());
    }

    #[test]
    fn a_region_variable_never_solves_to_a_type() {
        // The two tables are separate so this cannot even be written; the
        // test is here to fail loudly if they are ever merged.
        let mut u = Unifier::new();
        let ty = u.fresh();
        let region = u.fresh_region();
        assert!(matches!(ty, Type::Var(_)));
        assert!(matches!(region, Region::Var(_)));
    }

    #[test]
    fn the_referent_is_invariant() {
        // "`T` never changes" (§5.2). A variable inside a reference still
        // solves, but it solves to exactly what it met.
        let mut u = Unifier::new();
        let v = u.fresh();
        let hole =
            Type::Ref { unique: false, region: Region::Block(0), inner: Box::new(v.clone()) };
        let concrete =
            Type::Ref { unique: false, region: Region::Block(0), inner: Box::new(Type::Bool) };
        u.unify(&hole, &concrete).unwrap();
        assert_eq!(u.resolve(&v), Type::Bool);
    }

    #[test]
    fn escape_is_an_occurs_check() {
        // §5 rule 4, the whole of it.
        let inner =
            Type::Ref { unique: false, region: Region::Block(1), inner: Box::new(Type::Int) };
        let nested = Type::Named(DefId(0), vec![inner]);
        assert!(nested.mentions(Region::Block(1)));
        assert!(!nested.mentions(Region::Block(0)));
        assert!(!Type::Int.mentions(Region::Block(1)));
    }

    #[test]
    fn substitution_reaches_regions_and_types_together() {
        let signature = Type::Ref {
            unique: false,
            region: Region::Param(0),
            inner: Box::new(Type::Named(DefId(0), vec![Type::Param(0)])),
        };
        let instantiated = signature.substitute(&[Type::Bool], &[Region::Block(3)]);
        assert_eq!(
            instantiated,
            Type::Ref {
                unique: false,
                region: Region::Block(3),
                inner: Box::new(Type::Named(DefId(0), vec![Type::Bool])),
            }
        );
    }

    #[test]
    fn a_reference_renders_the_way_it_is_written() {
        let mut u = Unifier::new();
        let def = u.declare("File");
        u.set_region_block_names(vec!["r".to_owned()]);
        let shared = Type::Ref {
            unique: false,
            region: Region::Block(0),
            inner: Box::new(Type::Named(def, Vec::new())),
        };
        assert_eq!(u.display(&shared), "&r File");
        let unique = Type::Ref {
            unique: true,
            region: Region::Block(0),
            inner: Box::new(Type::Named(def, Vec::new())),
        };
        assert_eq!(u.display(&unique), "&!r File");
    }
}
