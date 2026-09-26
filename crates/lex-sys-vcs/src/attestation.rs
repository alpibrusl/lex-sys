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
//! **Seals, not checkpoints.** `lex-os-audit`'s own three layers (chain,
//! seals, checkpoints) name exactly what each closes and what it does
//! not: the chain alone catches an edited payload or a reordered entry,
//! not a rewrite by a holder who recomputes every hash after editing;
//! a seal (an Ed25519 signature over each entry's hash, via
//! `ed25519-dalek` — the same crate `lex-os-audit` uses, not
//! `std.ed25519`, which is for a different consumer, `docs/ed25519.md`
//! §1) closes that, because a holder without the key can still rewrite
//! the log but cannot re-sign it. Checkpoints — a signature over
//! `(domain, len, head)`, closing *truncation* — are not built here:
//! nothing in this crate yet holds a checkpoint anywhere the auditee
//! cannot reach, and a checkpoint with no such home would be a second
//! layer that closes nothing.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
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

/// Domain separator for an entry seal — distinct from the entry hash's
/// own domain (`AttestationEvent::DOMAIN`) so a signature over one can
/// never be read as the other.
const SEAL_DOMAIN: &[u8] = b"lex.sys.vcs.attestation.seal.v1";

/// An Ed25519 signature over one entry's hash, by a named signer.
///
/// Over the *hash*, not the payload: the hash already commits to the
/// payload, the sequence number and the predecessor, so signing it
/// commits to all three and to the entry's position in the chain.
/// Deliberately not part of [`Entry`]'s own hash (`compute_hash`) — if
/// the seal fed the hash, sealing an entry would change its hash, and
/// every already-written unsealed log would stop verifying the moment
/// sealing was turned on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seal {
    /// Hex-encoded Ed25519 public key (32 bytes) of whoever sealed it.
    pub signer: String,
    /// Hex-encoded Ed25519 signature (64 bytes) over the seal domain and
    /// the entry's own `hash`.
    pub signature: String,
}

fn seal_payload(hash: &str) -> Vec<u8> {
    let mut payload = SEAL_DOMAIN.to_vec();
    payload.extend_from_slice(hash.as_bytes());
    payload
}

/// One link in the chain: a sequence number, the previous entry's hash,
/// the event, and this entry's own hash over all of the above.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry<E> {
    pub seq: u64,
    pub prev_hash: String,
    pub event: E,
    pub hash: String,
    /// The seal over `hash`, when this entry was sealed. Optional, and
    /// omitted from the JSON entirely when absent (`skip_serializing_if`),
    /// so a log written before sealing was ever wired in still parses and
    /// still hashes to the same values — an unsealed entry is not a
    /// failure, it is a log nobody signed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seal: Option<Seal>,
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
#[derive(Clone, Serialize, Deserialize)]
pub struct Chain<E> {
    entries: Vec<Entry<E>>,
    /// The key that seals each entry as it is appended, when the owner
    /// set one. On the chain rather than in a wrapper, so whoever owns
    /// the log decides once and no `append` call site can forget to seal
    /// — `lex-os-audit`'s own reason for the same placement.
    ///
    /// Never serialized, never printed (see the hand-written `Debug`
    /// below), and never part of the log's identity (see the
    /// hand-written `PartialEq`): two chains with the same entries are
    /// the same log whoever holds the pen.
    #[serde(skip)]
    signing_key: Option<SigningKey>,
}

// Hand-written so a signing key can never reach a log line or a panic
// message.
impl<E: std::fmt::Debug> std::fmt::Debug for Chain<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Chain")
            .field("entries", &self.entries)
            .field("signing_key", &self.signing_key.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl<E: PartialEq> PartialEq for Chain<E> {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries
    }
}

impl<E: Eq> Eq for Chain<E> {}

impl<E: ChainPayload> Default for Chain<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: ChainPayload> Chain<E> {
    pub fn new() -> Self {
        Self { entries: Vec::new(), signing_key: None }
    }

    /// Seal every entry appended from here on. Entries already in the
    /// chain are left alone — re-sealing entries this holder did not
    /// write would be vouching for decisions it did not make.
    #[must_use]
    pub fn sealed_with(mut self, key: SigningKey) -> Self {
        self.signing_key = Some(key);
        self
    }

    /// Is this chain sealing what it appends?
    pub fn is_sealing(&self) -> bool {
        self.signing_key.is_some()
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

    /// Append one event, chaining it from the current head. Sealed with
    /// [`Self::sealed_with`]'s key, when one is set.
    pub fn append(&mut self, event: E) -> &Entry<E> {
        let seq = self.entries.len() as u64;
        let prev_hash = self.head();
        let hash = compute_hash(seq, &prev_hash, &event);
        let seal = self.signing_key.as_ref().map(|key| {
            let signature = key.sign(&seal_payload(&hash));
            Seal {
                signer: hex::encode(key.verifying_key().to_bytes()),
                signature: hex::encode(signature.to_bytes()),
            }
        });
        self.entries.push(Entry { seq, prev_hash, event, hash, seal });
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

    /// Does every entry carry a valid seal by `key`? An entry with no
    /// seal at all, or a seal naming a different signer, or a signature
    /// that does not verify, are each reported the same way this module
    /// reports everything else that fails — with which entry and why,
    /// not just that something did.
    ///
    /// An empty chain trivially passes: there is nothing to have been
    /// sealed wrong. Call [`Self::verify`] first regardless — a broken
    /// chain with valid-looking seals on the surviving entries is still
    /// a broken chain.
    pub fn verify_seals(&self, key: &VerifyingKey) -> Result<(), BrokenAt> {
        for entry in &self.entries {
            let seal = entry.seal.as_ref().ok_or_else(|| BrokenAt {
                seq: entry.seq,
                detail: "no seal on this entry".to_owned(),
            })?;
            let sig_bytes: [u8; 64] = hex::decode(&seal.signature)
                .ok()
                .and_then(|b| b.try_into().ok())
                .ok_or_else(|| BrokenAt {
                    seq: entry.seq,
                    detail: "seal signature is not 64 bytes of hex".to_owned(),
                })?;
            let signature = Signature::from_bytes(&sig_bytes);
            key.verify(&seal_payload(&entry.hash), &signature).map_err(|_| BrokenAt {
                seq: entry.seq,
                detail: "seal does not verify against the given key".to_owned(),
            })?;
        }
        Ok(())
    }
}
