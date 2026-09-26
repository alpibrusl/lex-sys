//! A content-addressed operation log for lex-sys programs.
//!
//! `docs/vcs.md` is the design; this is its foundation slice (§8): the
//! `Operation` vocabulary and its canonical, content-addressed identity,
//! with no gate, attestation, signing, merge or issue tracking on top of it
//! yet — those port from `lex-lang`'s `lex-vcs` largely unmodified (§3), and
//! nothing downstream needs building before this file exists to build it on.
//!
//! This crate shares `lex-vcs`'s *scheme* — `String`-keyed ids, canonical
//! JSON, a content hash of `(kind, sorted parents, edition)` — and no code,
//! the same relationship `lex-os` and `lex-sys` already committed to
//! (`README.md`'s "Where this sits").

mod canonical;
mod operation;

pub use canonical::canonical_bytes;
pub use operation::{
    EffectSet, ModuleRef, OpId, Operation, OperationKind, OperationRecord, SigId, StageId,
};
