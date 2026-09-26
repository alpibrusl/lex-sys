//! A hash-chained attestation log: what happened to each `Operation`
//! this repository's gate (`gate.rs`) was asked to check, recorded before
//! anything downstream trusts the answer.
//!
//! `docs/vcs.md` §3 named attestation as porting from `lex-vcs` largely
//! unmodified; reading `lex-os-audit`'s own `Chain<E>` (a sibling idea in
//! a sibling repo — a supervisor's tamper-evident log, not a VCS's) found
//! the same "generic hash chain over a payload type" shape already built
//! and already well-tested there. This is a **native** reimplementation
//! of that idea, not a dependency on either repository's crate: `lex-os`
//! and `lex-sys` already committed to sharing ideas and no code
//! (`README.md`), and the same rule applies a third time here, between
//! this file and `lex-os-audit`.
//!
//! **No signing yet** (`docs/vcs.md` §8's own ordering: signing needs
//! `std.crypto`'s Ed25519, which does not exist — `docs/sha512.md` §5).
//! `lex-os-audit`'s own three layers (chain, seals, checkpoints) name
//! exactly what a chain alone does not close: a holder who can edit the
//! log can recompute every hash after the edit. This file is that first,
//! weakest layer, honestly labeled as such rather than presented as more
//! than it is.

use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// A hash chain's payload must be able to name its own domain separator,
/// so an entry from one vocabulary can never be replayed as an entry in
/// another — `lex-os-audit`'s own `ChainPayload` trait, ported as an
/// idea.
pub trait ChainPayload: Serialize + DeserializeOwned {
    const DOMAIN: &'static [u8];
}

/// What this log actually attests to: the one decision `gate.rs` makes,
/// recorded either way. Scoped to exactly that — no `ProducerTrust`, no
/// cost, no review verdicts; `lex-vcs`'s own broader `AttestationKind`
/// is ecosystem-layer vocabulary (an orchestrator's concerns) that has
/// no asker here yet (`AGENTS.md` §7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AttestationEvent {
    /// The candidate program `op_id` describes typechecked, and the op
    /// was (or is about to be) written to the op log.
    GateAccepted { op_id: crate::OpId },
    /// The candidate did not typecheck. `rules` is every refusal's rule
    /// tag (`gate::GateDiagnostic::rule`), so a reader of the chain alone
    /// — no access to whatever produced the candidate — can already see
    /// *why*, the same reason `agent-errors.md` gives a rule its own tag
    /// beside the sentence.
    GateRejected { op_id: crate::OpId, rules: Vec<String> },
}

impl ChainPayload for AttestationEvent {
    const DOMAIN: &'static [u8] = b"lex.sys.vcs.attestation.v1";
}

/// The hash chain's fixed starting point, so an empty log has a defined
/// head to chain the first entry from — 64 hex zeros, the width of a
/// BLAKE3 digest, the same shape `lex-os-audit`'s own `GENESIS` picks for
/// a SHA-256 digest.
pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// One link in the chain: a sequence number, the previous entry's hash,
/// the event, and this entry's own hash over all of the above.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry<E> {
    pub seq: u64,
    pub prev_hash: String,
    pub event: E,
    pub hash: String,
}

fn compute_hash<E: ChainPayload>(seq: u64, prev_hash: &str, event: &E) -> String {
    let event_json = serde_json::to_string(event).expect("an AttestationEvent always serializes");
    let mut hasher = blake3::Hasher::new();
    hasher.update(E::DOMAIN);
    hasher.update(&seq.to_be_bytes());
    hasher.update(prev_hash.as_bytes());
    hasher.update(event_json.as_bytes());
    hasher.finalize().to_hex().to_string()
}

/// Why [`Chain::verify`] found a broken link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokenAt {
    pub seq: u64,
    pub detail: String,
}

/// An append-only, hash-chained log, generic over its payload the same
/// way `lex-os-audit`'s `Chain<E>` is. No instantiation gets an edit or
/// truncate API — append-only is a property of the type, not a
/// convention a caller has to remember.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chain<E> {
    entries: Vec<Entry<E>>,
}

impl<E: ChainPayload> Default for Chain<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: ChainPayload> Chain<E> {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// The hash at the head of the chain — [`GENESIS`] when empty.
    pub fn head(&self) -> String {
        self.entries.last().map(|e| e.hash.clone()).unwrap_or_else(|| GENESIS.to_string())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[Entry<E>] {
        &self.entries
    }

    /// Append one event, chaining it from the current head.
    pub fn append(&mut self, event: E) -> &Entry<E> {
        let seq = self.entries.len() as u64;
        let prev_hash = self.head();
        let hash = compute_hash(seq, &prev_hash, &event);
        self.entries.push(Entry { seq, prev_hash, event, hash });
        self.entries.last().expect("just pushed")
    }

    /// Walk every entry and recompute its hash from its own contents,
    /// checking it against both what the entry claims and what the next
    /// entry's `prev_hash` claims came before it. Catches an edited
    /// payload, a reordered entry, and a deletion from anywhere but the
    /// tail — `lex-os-audit`'s own documented limits on what a bare
    /// chain proves apply here unchanged, most importantly that a
    /// **truncated** chain still verifies, because nothing inside a log
    /// can prove a further entry once followed it.
    pub fn verify(&self) -> Result<(), BrokenAt> {
        let mut prev_hash = GENESIS.to_string();
        for (i, entry) in self.entries.iter().enumerate() {
            let seq = i as u64;
            if entry.seq != seq {
                return Err(BrokenAt {
                    seq,
                    detail: format!("expected sequence number {seq}, found {}", entry.seq),
                });
            }
            if entry.prev_hash != prev_hash {
                return Err(BrokenAt {
                    seq,
                    detail: "prev_hash does not match the preceding entry's hash".to_owned(),
                });
            }
            let recomputed = compute_hash(entry.seq, &entry.prev_hash, &entry.event);
            if recomputed != entry.hash {
                return Err(BrokenAt {
                    seq,
                    detail: "hash does not match its own contents".to_owned(),
                });
            }
            prev_hash = entry.hash.clone();
        }
        Ok(())
    }
}
