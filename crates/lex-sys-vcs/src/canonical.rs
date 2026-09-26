//! Canonical encoding and content-addressed identity for an [`Operation`].
//!
//! `docs/canonical-ast.md` picked BLAKE3 for `lex-sys-id`'s own per-unit
//! hashes and said why: nothing here makes that reasoning specific to an
//! AST node rather than an operation, so this file reuses the same
//! algorithm rather than pulling in a second hash dependency (`sha2`, which
//! `lex-vcs` uses) to match `lex-vcs`'s choice for no reason but appearance
//! — `docs/vcs.md` §5's own rule, "shares the idea and no code," applies to
//! which hash function as much as to which crate.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::operation::{OpId, Operation, OperationKind};

/// A transient, hashable view of an [`Operation`] with `parents` collected
/// into a `BTreeSet` — so the serialization is canonical regardless of the
/// order [`Operation::new`] was given them in, the same reason `lex-vcs`'s
/// own `CanonicalView` exists. Never persisted.
#[derive(Serialize)]
struct CanonicalView<'a> {
    #[serde(flatten)]
    kind: &'a OperationKind,
    parents: BTreeSet<&'a OpId>,
    edition: u32,
}

/// The exact bytes hashed to produce an operation's [`OpId`]. Exposed
/// separately from [`op_id`] so a future golden-fixture file (the
/// `lex-sys-id` precedent: `docs/canonical-ast.md` §8, `crates/lex-sys-id/
/// tests/golden.rs`) can pin the pre-image rather than only the digest.
pub fn canonical_bytes(op: &Operation) -> Vec<u8> {
    let view =
        CanonicalView { kind: &op.kind, parents: op.parents.iter().collect(), edition: op.edition };
    serde_json::to_vec(&view).expect("an Operation always serializes")
}

/// `op.op_id()`'s implementation: BLAKE3 of [`canonical_bytes`], lowercase
/// hex, the same rendering `lex-sys-id::Hash::to_hex` uses.
pub fn op_id(op: &Operation) -> OpId {
    blake3::hash(&canonical_bytes(op)).to_hex().to_string()
}
