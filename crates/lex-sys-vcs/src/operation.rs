//! The `Operation` enum and content-addressed identity.
//!
//! `docs/vcs.md` §3 is the design: reuse `lex-lang`'s `lex-vcs` scheme with
//! no shared code (§5), keyed on the same `String`/`BTreeSet<String>` idea
//! that crate's own `operation.rs` states as deliberate — *"we keep it as
//! `String` here so this crate has no dependency on `lex-store`'s
//! internals"* — so this file has no dependency on `lex-sys-id`'s `Hash`
//! type either. `SigId`/`StageId` are `lex-sys-id`'s own hex-encoded hashes,
//! read as plain strings; `EffectSet` is the row `lex-sys authority` already
//! reports, as strings.
//!
//! What is here is the **foundation** `docs/vcs.md` §8 named: enough to
//! construct, canonicalise and hash an `Operation`, checked against a golden
//! case. The gate (`gate.rs`'s idea — run the checker against the candidate
//! program before an op is accepted), attestation, signing, merge and issue
//! tracking are §3's claim that they port unmodified; none of them are built
//! here, because none of them have anything to attach to yet without this
//! file existing first.

use std::collections::BTreeSet;

use serde::Serialize;

/// Signature identity of a function — what a caller depends on. One of
/// `lex-sys-id`'s two hashes per function (`FunctionId::sig`), read as its
/// hex string rather than its `Hash` type, for the same reason `lex-vcs`
/// gives for its own identical-looking alias: this crate should not need to
/// change if `lex-sys-id`'s internal representation ever does.
pub type SigId = String;

/// Content hash of one implementation of a function — `lex-sys-id`'s
/// `FunctionId::body`, as a hex string. Named `StageId` rather than `BodyId`
/// to match `lex-vcs`'s own vocabulary for "one committed version of a
/// body," which is the concept an op DAG actually manipulates.
pub type StageId = String;

/// Identity of an operation: the BLAKE3 hash, lowercase hex, of its own
/// canonical encoding (`canonical::op_id`). Two operations with identical
/// payloads, parents and edition produce identical `OpId`s.
pub type OpId = String;

/// Sorted set of effect-label strings — the same row shape
/// `lex-sys authority --output json` already emits, and the same shape
/// `EffectSet` has in `lex-vcs`. A `BTreeSet` so the canonical form is
/// order-independent for hashing, which a `Vec` would not be.
pub type EffectSet = BTreeSet<String>;

/// The declaration this operation is about, spelled the way `AGENTS.md`
/// spells one: `many-files.md`'s identity-by-content model means this is a
/// name for a human to read, never part of what gets hashed.
pub type ModuleRef = String;

/// A typed delta on a lex-sys program. `docs/vcs.md` §4 scopes this
/// vocabulary deliberately narrower than `lex-vcs`'s: no `budget_cost`
/// field on `AddFunction`/`ModifyBody`, because `budget.md` settled
/// `[budget]` as a language feature with a documented **no** — there is
/// nothing for such a field to ever hold here, so it is not declared rather
/// than declared and permanently `None`. No signature-changing `ModifyBody`
/// case yet either (`lex-vcs`'s `to_sig_id`, #992): nothing in this
/// repository has asked for one, and `AGENTS.md` §7's own rule is to build
/// what a program asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind")]
pub enum OperationKind {
    /// A new function, published for the first time.
    AddFunction {
        sig_id: SigId,
        stage_id: StageId,
        effects: EffectSet,
        /// The source file this declaration came from, when the program is
        /// more than one file (`many-files.md`). `None` for a single-file
        /// program, so those ops serialize identically with or without this
        /// field ever existing — the same additive trick `lex-vcs` uses for
        /// its own optional fields.
        #[serde(skip_serializing_if = "Option::is_none")]
        in_file: Option<ModuleRef>,
    },
    /// A function removed. `last_stage_id` is the head before the removal,
    /// so a later reader can walk to the predecessor without a scan.
    RemoveFunction { sig_id: SigId, last_stage_id: StageId },
    /// A function's body changed; its signature — and so its `SigId` — did
    /// not.
    ModifyBody { sig_id: SigId, from_stage_id: StageId, to_stage_id: StageId },
}

/// One operation and the causal predecessors it assumes.
///
/// `edition` is `Ast::edition_of`'s own `u32` (`editions.md`): the
/// vocabulary `effects` was written against. `docs/vcs.md` §6's own
/// argument — tag it from the day the DAG is born, not once a second
/// edition already exists to distinguish from the first — is why this
/// field is not `Option`-and-added-later the way `lex-vcs`'s `intent_id`
/// was: there is exactly one plateau this initiative is starting from
/// (`hash-stability.md`), and it is edition 1 today.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Operation {
    #[serde(flatten)]
    pub kind: OperationKind,
    /// Operations this one assumes. Sorted and deduplicated before hashing
    /// (`canonical::op_id`), regardless of the order passed to
    /// [`Operation::new`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parents: Vec<OpId>,
    pub edition: u32,
}

impl Operation {
    /// Construct an operation against zero or more parents, in any order —
    /// [`crate::canonical::op_id`] sorts and deduplicates them before
    /// hashing.
    pub fn new(kind: OperationKind, edition: u32, parents: impl IntoIterator<Item = OpId>) -> Self {
        Self { kind, parents: parents.into_iter().collect(), edition }
    }

    /// This operation's content-addressed identity.
    pub fn op_id(&self) -> OpId {
        crate::canonical::op_id(self)
    }
}

/// An operation paired with its computed [`OpId`] — what a store persists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRecord {
    pub op_id: OpId,
    pub operation: Operation,
}

impl OperationRecord {
    pub fn new(operation: Operation) -> Self {
        Self { op_id: operation.op_id(), operation }
    }
}
