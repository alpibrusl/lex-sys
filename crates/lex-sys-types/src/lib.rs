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

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Type {
    Int,
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
    /// An unsolved inference variable.
    Var(TyVar),
}

impl Type {
    /// Does `var` occur anywhere in this type? The check that keeps
    /// unification from building an infinite type.
    pub fn occurs(&self, var: TyVar) -> bool {
        match self {
            Type::Var(v) => *v == var,
            Type::Named(_, args) => args.iter().any(|a| a.occurs(var)),
            _ => false,
        }
    }

    pub fn is_var(&self) -> bool {
        matches!(self, Type::Var(_))
    }

    /// Replace each `Param(i)` with `args[i]`.
    pub fn substitute(&self, args: &[Type]) -> Type {
        match self {
            Type::Param(i) => args
                .get(*i as usize)
                .cloned()
                .unwrap_or_else(|| panic!("no argument for type parameter {i}")),
            Type::Named(def, inner) => {
                Type::Named(*def, inner.iter().map(|t| t.substitute(args)).collect())
            }
            other => other.clone(),
        }
    }
}

/// Why two types could not be made equal.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum UnifyError {
    /// The two types are different and neither is a variable.
    Mismatch { expected: Type, found: Type },
    /// Solving would build an infinite type, as in `T = Pair[T, T]`.
    Infinite { var: TyVar, ty: Type },
}

/// A union-find over inference variables.
#[derive(Default, Debug)]
pub struct Unifier {
    /// `None` while unsolved; solving is one assignment and is never undone.
    solved: Vec<Option<Type>>,
    /// Names for rendering a `Named` type in a diagnostic. The driver supplies
    /// them; this crate never invents one.
    names: Vec<String>,
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

    pub fn fresh(&mut self) -> Type {
        self.solved.push(None);
        Type::Var(TyVar(self.solved.len() as u32 - 1))
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
            Type::Named(def, args) => {
                Type::Named(def, args.iter().map(|a| self.resolve(a)).collect())
            }
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
            Type::Bool => "bool".to_owned(),
            Type::Unit => "()".to_owned(),
            Type::Param(i) => format!("T{i}"),
            Type::Var(v) => format!("?{}", v.0),
            Type::Named(def, args) if args.is_empty() => self.name_of(def).to_owned(),
            Type::Named(def, args) => {
                let inner: Vec<String> = args.iter().map(|a| self.display(a)).collect();
                format!("{}[{}]", self.name_of(def), inner.join(", "))
            }
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
        let applied = generic.substitute(&[Type::Int, Type::Bool]);
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
}
