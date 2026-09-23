//! The rule catalogue: a stable name for every refusal.
//!
//! `docs/agent-errors.md`. A [`Diagnostic`](crate::Diagnostic) carries a
//! sentence for a person and a [`Rule`] for a program, and §3 is the line
//! between them: **a tag names the rule a reader would look up, not the
//! sentence the checker wrote.** Two refusals that break the same rule
//! share a tag however differently they are worded.
//!
//! §1 is why this exists. Across 196 must-reject fixtures the checker
//! produced 125 distinct message shapes, 101 of them seen exactly once —
//! so a consumer classifying a refusal had 125 English patterns to match
//! and no vocabulary at all. These are the 53 rules underneath them,
//! and `internal` (`docs/internal-errors.md`), which names the
//! compiler's failures rather than the program's.
//!
//! **A tag is stable.** Once shipped it never changes meaning: a new
//! rule gets a new tag, and a rule that splits gets siblings rather than
//! repurposing its parent. `canonical-ast.md`'s argument about hashes,
//! applied to a name — it is worth something only if it actually holds
//! still.

/// The rule a refusal enforces.
///
/// Every variant is reachable from at least one error site, and — after
/// `every_rule_has_a_fixture` — from at least one fixture under
/// `tests/reject/`. §3.2: counting the rules is what made that claim
/// checkable, and it was false for nine of them when it was first
/// counted.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Rule {
    AmbiguousType,
    ArityMismatch,
    AssignToImmutable,
    BorrowConflict,
    BuiltinRedeclared,
    CapabilityMisused,
    CapabilityNotNarrowable,
    ConstantTraps,
    DuplicateDeclaration,
    EffectDeclaredNotPerformed,
    EffectNotDeclared,
    EnumHasNoVariants,
    FieldOrder,
    ForeignBoundaryType,
    ForeignDeclaration,
    InfiniteType,
    /// The compiler failed on a program it had accepted
    /// (`docs/internal-errors.md`). No fixture reaches it on purpose.
    Internal,
    LinearUseAfterMove,
    LinearValueTakenApart,
    LinearValueUnconsumed,
    LiteralForm,
    LiteralOutOfRange,
    MatchArmUnreachable,
    MatchNotExhaustive,
    MatchOnANonEnum,
    MissingField,
    MissingReturn,
    ModeBoundViolated,
    ModuleNotImported,
    NoFunctionValues,
    NotAFunction,
    NotAPlace,
    NotAReference,
    NotASlice,
    NotAStruct,
    NotATuple,
    NotAnEnum,
    NotPublic,
    OperatorTypeMismatch,
    PatternShape,
    ProgramShape,
    ReferenceEscapesRegion,
    RegionMismatch,
    RegionNotInScope,
    SharedReferenceWritten,
    StaticItem,
    TypeArgsNotTaken,
    TypeMismatch,
    UnexpectedCharacter,
    UnknownEdition,
    UnknownEscape,
    UnknownName,
    UnreachableStatement,
    UnsizedType,
}

impl Rule {
    /// Every rule, in tag order. The catalogue as data.
    pub const ALL: [Rule; 54] = [
        Rule::AmbiguousType,
        Rule::ArityMismatch,
        Rule::AssignToImmutable,
        Rule::BorrowConflict,
        Rule::BuiltinRedeclared,
        Rule::CapabilityMisused,
        Rule::CapabilityNotNarrowable,
        Rule::ConstantTraps,
        Rule::DuplicateDeclaration,
        Rule::EffectDeclaredNotPerformed,
        Rule::EffectNotDeclared,
        Rule::EnumHasNoVariants,
        Rule::FieldOrder,
        Rule::ForeignBoundaryType,
        Rule::ForeignDeclaration,
        Rule::InfiniteType,
        Rule::Internal,
        Rule::LinearUseAfterMove,
        Rule::LinearValueTakenApart,
        Rule::LinearValueUnconsumed,
        Rule::LiteralForm,
        Rule::LiteralOutOfRange,
        Rule::MatchArmUnreachable,
        Rule::MatchNotExhaustive,
        Rule::MatchOnANonEnum,
        Rule::MissingField,
        Rule::MissingReturn,
        Rule::ModeBoundViolated,
        Rule::ModuleNotImported,
        Rule::NoFunctionValues,
        Rule::NotAFunction,
        Rule::NotAPlace,
        Rule::NotAReference,
        Rule::NotASlice,
        Rule::NotAStruct,
        Rule::NotATuple,
        Rule::NotAnEnum,
        Rule::NotPublic,
        Rule::OperatorTypeMismatch,
        Rule::PatternShape,
        Rule::ProgramShape,
        Rule::ReferenceEscapesRegion,
        Rule::RegionMismatch,
        Rule::RegionNotInScope,
        Rule::SharedReferenceWritten,
        Rule::StaticItem,
        Rule::TypeArgsNotTaken,
        Rule::TypeMismatch,
        Rule::UnexpectedCharacter,
        Rule::UnknownEdition,
        Rule::UnknownEscape,
        Rule::UnknownName,
        Rule::UnreachableStatement,
        Rule::UnsizedType,
    ];

    /// The stable kebab-case name. Never changes meaning once shipped.
    pub fn tag(self) -> &'static str {
        match self {
            Rule::AmbiguousType => "ambiguous-type",
            Rule::ArityMismatch => "arity-mismatch",
            Rule::AssignToImmutable => "assign-to-immutable",
            Rule::BorrowConflict => "borrow-conflict",
            Rule::BuiltinRedeclared => "builtin-redeclared",
            Rule::CapabilityMisused => "capability-misused",
            Rule::CapabilityNotNarrowable => "capability-not-narrowable",
            Rule::ConstantTraps => "constant-traps",
            Rule::DuplicateDeclaration => "duplicate-declaration",
            Rule::EffectDeclaredNotPerformed => "effect-declared-not-performed",
            Rule::EffectNotDeclared => "effect-not-declared",
            Rule::EnumHasNoVariants => "enum-has-no-variants",
            Rule::FieldOrder => "field-order",
            Rule::ForeignBoundaryType => "foreign-boundary-type",
            Rule::ForeignDeclaration => "foreign-declaration",
            Rule::InfiniteType => "infinite-type",
            Rule::Internal => "internal",
            Rule::LinearUseAfterMove => "linear-use-after-move",
            Rule::LinearValueTakenApart => "linear-value-taken-apart",
            Rule::LinearValueUnconsumed => "linear-value-unconsumed",
            Rule::LiteralForm => "literal-form",
            Rule::LiteralOutOfRange => "literal-out-of-range",
            Rule::MatchArmUnreachable => "match-arm-unreachable",
            Rule::MatchNotExhaustive => "match-not-exhaustive",
            Rule::MatchOnANonEnum => "match-on-a-non-enum",
            Rule::MissingField => "missing-field",
            Rule::MissingReturn => "missing-return",
            Rule::ModeBoundViolated => "mode-bound-violated",
            Rule::ModuleNotImported => "module-not-imported",
            Rule::NoFunctionValues => "no-function-values",
            Rule::NotAFunction => "not-a-function",
            Rule::NotAPlace => "not-a-place",
            Rule::NotAReference => "not-a-reference",
            Rule::NotASlice => "not-a-slice",
            Rule::NotAStruct => "not-a-struct",
            Rule::NotATuple => "not-a-tuple",
            Rule::NotAnEnum => "not-an-enum",
            Rule::NotPublic => "not-public",
            Rule::OperatorTypeMismatch => "operator-type-mismatch",
            Rule::PatternShape => "pattern-shape",
            Rule::ProgramShape => "program-shape",
            Rule::ReferenceEscapesRegion => "reference-escapes-region",
            Rule::RegionMismatch => "region-mismatch",
            Rule::RegionNotInScope => "region-not-in-scope",
            Rule::SharedReferenceWritten => "shared-reference-written",
            Rule::StaticItem => "static-item",
            Rule::TypeArgsNotTaken => "type-args-not-taken",
            Rule::TypeMismatch => "type-mismatch",
            Rule::UnexpectedCharacter => "unexpected-character",
            Rule::UnknownEdition => "unknown-edition",
            Rule::UnknownEscape => "unknown-escape",
            Rule::UnknownName => "unknown-name",
            Rule::UnreachableStatement => "unreachable-statement",
            Rule::UnsizedType => "unsized-type",
        }
    }

    /// What the rule enforces, independent of the occurrence that
    /// tripped it.
    ///
    /// One sentence per rule rather than per error, short enough to
    /// inline in a repair prompt and specific enough to suggest the next
    /// move. lex-lang calls this `rule_explanation` and the shape is
    /// taken from it.
    pub fn explanation(self) -> &'static str {
        match self {
            Rule::AmbiguousType => {
                "A type the arguments do not settle must be written down; inference here is local \
                 and total, and never guesses."
            }
            Rule::ArityMismatch => {
                "A call, a pattern or a variant takes a fixed number of things, and this gave a \
                 different number."
            }
            Rule::AssignToImmutable => {
                "A `let` binding never changes after it is bound; `var` is the declaration that \
                 allows assignment."
            }
            Rule::BorrowConflict => {
                "A unique reference `&!` is the only way to reach its referent while it lives, so \
                 nothing else may read, write, move or consume that value meanwhile."
            }
            Rule::BuiltinRedeclared => {
                "The prelude's names — the capabilities, the builtins, the primitive types — \
                 belong to the language and a program may not redefine one."
            }
            Rule::CapabilityMisused => {
                "A capability is a value with exactly one way to use it: hold it, lend it, or end \
                 it with `release`. It is not taken apart, not written as a literal, and an \
                 operation it authorises reaches it through the parameter that names it."
            }
            Rule::CapabilityNotNarrowable => {
                "`narrow` attenuates and never widens: the target must be strictly inside what the \
                 capability already grants, spelled as a literal so it can be checked where it is \
                 written."
            }
            Rule::ConstantTraps => {
                "An operation whose operands are all literals is evaluated during compilation, so \
                 one that would trap at run time cannot be compiled at all."
            }
            Rule::DuplicateDeclaration => {
                "A name is introduced once in the scope that holds it — a file, a signature, a \
                 struct, a pattern or a literal."
            }
            Rule::EffectDeclaredNotPerformed => {
                "An effect row is exact in both directions: a label a function declares but never \
                 performs makes the row decoration rather than a contract."
            }
            Rule::EffectNotDeclared => {
                "Every effect a function's body performs appears in its row, and a function \
                 performs what the capability it was lent authorises."
            }
            Rule::EnumHasNoVariants => {
                "An enum with no variants has no values, so nothing can ever construct or match one."
            }
            Rule::FieldOrder => {
                "A struct literal's fields are evaluated in declaration order, so writing them in \
                 another order would hide which expression runs first."
            }
            Rule::ForeignBoundaryType => {
                "A foreign parameter or result must have a layout both sides agree on: the \
                 scalars, and a slice by reference. Nothing else crosses."
            }
            Rule::ForeignDeclaration => {
                "A foreign declaration names one library and one symbol, and that pairing is \
                 unique in a program."
            }
            Rule::InfiniteType => {
                "A type that contains itself has no finite size, so a `Box` has to sit somewhere on \
                 the path back to it."
            }
            Rule::Internal => {
                "The compiler failed on a program it had accepted. The program is not at fault; the \
                 position is the function whose code could not be generated."
            }
            Rule::LinearUseAfterMove => {
                "A `res` value is used exactly once. Once it has been moved or consumed there is \
                 nothing left to use, borrow or read."
            }
            Rule::LinearValueTakenApart => {
                "A `res` value is ended by the function that owns that job — `release`, `unbox`, \
                 a destructuring `let` — and never by reading a part out of it, which would leave \
                 the rest unaccounted for."
            }
            Rule::LinearValueUnconsumed => {
                "A `res` value is consumed exactly once on every path: not dropped at the end of a \
                 block, not discarded by an assignment or a statement, and not left to differ \
                 between branches."
            }
            Rule::LiteralForm => {
                "A literal has one spelling — a string that does not span lines, a `0x` with digits \
                 after it, a float with a point — and this is not it."
            }
            Rule::LiteralOutOfRange => {
                "A literal must fit the type it is written at: `int` is 64-bit signed and `float` \
                 is IEEE-754 binary64."
            }
            Rule::MatchArmUnreachable => {
                "Every arm of a `match` must be able to run: no arm after one that already covers \
                 everything, and no variant matched twice."
            }
            Rule::MatchNotExhaustive => {
                "A `match` covers every variant of its enum, or has a `_`. There is no runtime \
                 failure for a value that matched nothing."
            }
            Rule::MatchOnANonEnum => {
                "`match` takes an enum or a reference to one. A struct is taken apart by \
                 destructuring instead."
            }
            Rule::MissingField => {
                "A struct literal gives every field: there is no default and no partial value."
            }
            Rule::MissingReturn => {
                "Every path out of a function returns a value of its declared type; falling off the \
                 end is not one of them."
            }
            Rule::ModeBoundViolated => {
                "`res` and `val` are the two modes, and a container, a parameter or a declaration \
                 that promises `val` cannot hold a `res`: copying one would leave two values owing \
                 one obligation."
            }
            Rule::ModuleNotImported => {
                "A qualified name reaches another module only where this file has imported it."
            }
            Rule::NoFunctionValues => {
                "A function name can be called and nothing else: there are no function values."
            }
            Rule::NotAFunction => {
                "The name in call position is not a function in this program: a local binding is a \
                 value, not something to call."
            }
            Rule::NotAPlace => {
                "Assignment writes to a place — a binding, or a field reached through a unique \
                 reference — and not to an arbitrary expression."
            }
            Rule::NotAReference => {
                "This operation follows or writes through a reference, and the value it was given \
                 is not one."
            }
            Rule::NotASlice => "Indexing and range-taking need a slice, and this value is not one.",
            Rule::NotAStruct => {
                "Field access, a struct literal or a destructuring `let` needs a struct, and this \
                 is not one."
            }
            Rule::NotATuple => {
                "A positional component or a `let (..)` needs a tuple, and this is not one."
            }
            Rule::NotAnEnum => {
                "A variant is constructed or matched from an enum, and the name qualifying it here \
                 is not one."
            }
            Rule::NotPublic => {
                "`pub` is what makes a declaration reachable from another module, and a `pub` \
                 signature may not name something that is not."
            }
            Rule::OperatorTypeMismatch => {
                "An operator is defined on the types that have the operation: arithmetic and \
                 ordering on the numbers, equality on the scalars."
            }
            Rule::PatternShape => {
                "A pattern says what it takes apart in one step, and the shapes it may take are \
                 fixed: there is no `()`, and a pattern does not nest."
            }
            Rule::ProgramShape => {
                "A program has one `main` taking the `World` and returning `int`, and a file \
                 declares at most one module, first."
            }
            Rule::ReferenceEscapesRegion => {
                "A reference may not outlive the region it points into — an arena, a `borrow` \
                 block, or a caller's region parameter."
            }
            Rule::RegionMismatch => {
                "Two regions are the same one or they are not, and a `where` clause is how a \
                 signature relates the ones it takes."
            }
            Rule::RegionNotInScope => {
                "A region comes from a `[&r]` parameter or a `region r { .. }` block, and this \
                 name is neither."
            }
            Rule::SharedReferenceWritten => {
                "A shared reference `&` promises its referent will not change, so it cannot be \
                 written through or turned into a unique one."
            }
            Rule::StaticItem => {
                "A `static` is compile-time data: a slice of scalars, evaluated during \
                 compilation, performing nothing and reading only `static`s declared before it."
            }
            Rule::TypeArgsNotTaken => {
                "A type parameter stands for one type and is not itself generic, so it takes no \
                 type arguments."
            }
            Rule::TypeMismatch => {
                "An expression's type does not match what the context requires — a return type, \
                 an annotation, a parameter, or the mode of a reference."
            }
            Rule::UnexpectedCharacter => {
                "A character that begins no token in this language, or that cannot appear where it \
                 does."
            }
            Rule::UnknownEdition => {
                "A file's `edition N;` marker names one of the editions this compiler knows; a \
                 file with no marker is edition 1, and there is nothing later to name yet."
            }
            Rule::UnknownEscape => {
                "A string literal takes the six escapes `\\n`, `\\r`, `\\t`, `\\\\`, `\\\"` and \
                 `\\0`; there is no `\\x` and no `\\u`, and a source file is already UTF-8."
            }
            Rule::UnknownName => {
                "The name is not in scope, or the thing it is read out of does not have it."
            }
            Rule::UnreachableStatement => {
                "A statement after one that always leaves the block — a `return`, or a loop that \
                 never ends — can never run."
            }
            Rule::UnsizedType => {
                "A slice type `[T]` has no size of its own, so it is used through a reference \
                 rather than as a value."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_rule_is_in_all_exactly_once() {
        // `ALL` is the catalogue as data — `check --output json`'s
        // consumers and `every_rule_has_a_fixture` both walk it, so a
        // variant missing from it is a rule that quietly stops being
        // counted.
        let tags: BTreeSet<&str> = Rule::ALL.iter().map(|r| r.tag()).collect();
        assert_eq!(tags.len(), Rule::ALL.len(), "two rules share a tag, or one is listed twice");
    }

    #[test]
    fn every_tag_is_kebab_case() {
        // The tag is the stable identifier a consumer matches on, so its
        // shape is part of the contract rather than a convention.
        for rule in Rule::ALL {
            let tag = rule.tag();
            assert!(!tag.is_empty(), "{rule:?} has an empty tag");
            assert!(
                tag.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "`{tag}` is not kebab-case"
            );
            assert!(!tag.starts_with('-') && !tag.ends_with('-'), "`{tag}` has a loose dash");
        }
    }

    #[test]
    fn every_rule_explains_itself() {
        // An explanation is what a consumer reads instead of the
        // occurrence, so an empty one is worse than none: it looks like
        // an answer.
        for rule in Rule::ALL {
            let text = rule.explanation();
            assert!(text.len() > 40, "{rule:?}'s explanation is too short to be one: {text:?}");
            assert!(text.ends_with('.'), "{rule:?}'s explanation is not a sentence: {text:?}");
        }
    }

    #[test]
    fn tags_are_sorted() {
        // `ALL` is in tag order so a listing is stable and a new rule
        // lands where a reader would look for it.
        let tags: Vec<&str> = Rule::ALL.iter().map(|r| r.tag()).collect();
        let mut sorted = tags.clone();
        sorted.sort_unstable();
        assert_eq!(tags, sorted, "`Rule::ALL` is not in tag order");
    }
}
