//! Persistence for the operation log: one JSON file per accepted
//! [`OperationRecord`], keyed by its own [`OpId`] (`docs/vcs.md` §8's
//! second slice).
//!
//! Loose-file only. `lex-vcs`'s own `op_log.rs` consolidates past ~10k
//! ops into content-addressed packfiles, and says so in its own doc
//! comment — exactly the scaling work `AGENTS.md` §7 says to build once
//! a real corpus needs it, not ahead of one existing.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::operation::{OpId, OperationRecord};

pub struct OpLog {
    dir: PathBuf,
}

/// Why [`OpLog::get`] refused a record that was actually on disk, rather
/// than simply not finding one.
#[derive(Debug)]
pub enum OpLogError {
    Io(io::Error),
    /// The bytes at `<op_id>.json` parsed, but did not deserialize into an
    /// `OperationRecord` — on-disk corruption or a hand-edited file.
    Malformed(serde_json::Error),
    /// The file parsed, but its own `op_id` field disagrees with
    /// `operation.op_id()` recomputed from its payload. Content addressing
    /// makes this the one error that must never be silently trusted: a
    /// record that lies about its own identity is not the record its
    /// filename claims to be.
    IdentityMismatch {
        claimed: OpId,
        recomputed: OpId,
    },
}

impl From<io::Error> for OpLogError {
    fn from(e: io::Error) -> Self {
        OpLogError::Io(e)
    }
}

impl std::fmt::Display for OpLogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpLogError::Io(e) => write!(f, "{e}"),
            OpLogError::Malformed(e) => write!(f, "malformed operation record: {e}"),
            OpLogError::IdentityMismatch { claimed, recomputed } => {
                write!(f, "record claims OpId {claimed} but its payload hashes to {recomputed}")
            }
        }
    }
}

impl std::error::Error for OpLogError {}

impl OpLog {
    /// Open (creating if absent) the operation log under `<root>/ops/`.
    pub fn open(root: &Path) -> io::Result<Self> {
        let dir = root.join("ops");
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    fn path(&self, op_id: &OpId) -> PathBuf {
        self.dir.join(format!("{op_id}.json"))
    }

    /// Persist a record. Idempotent: writing an already-present `OpId` is
    /// a no-op — content addressing means the bytes can only ever match,
    /// so there is nothing a second write could correct.
    ///
    /// Atomic via a same-directory temp file plus rename, so a concurrent
    /// reader never observes a partially-written file. `record.op_id` is
    /// not trusted here; a caller builds one through
    /// [`OperationRecord::new`], which computes it, so the one place this
    /// could disagree is a record built by hand — and `get` re-checks on
    /// the read side regardless, which is where a mismatch can actually
    /// do harm.
    pub fn put(&self, record: &OperationRecord) -> io::Result<()> {
        let path = self.path(&record.op_id);
        if path.exists() {
            return Ok(());
        }
        let bytes = serde_json::to_vec_pretty(record)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let tmp = self.dir.join(format!("{}.tmp", record.op_id));
        fs::write(&tmp, &bytes)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Read a record back, or `Ok(None)` if no such `OpId` is in the log.
    ///
    /// Recomputes `op_id` from the payload and refuses a disagreement
    /// (`OpLogError::IdentityMismatch`) rather than returning a record
    /// under a name it may not actually have — the read-side half of what
    /// makes this a content-addressed store rather than a directory of
    /// files that happen to be named after their contents.
    pub fn get(&self, op_id: &OpId) -> Result<Option<OperationRecord>, OpLogError> {
        let bytes = match fs::read(self.path(op_id)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let record: OperationRecord =
            serde_json::from_slice(&bytes).map_err(OpLogError::Malformed)?;
        let recomputed = record.operation.op_id();
        if recomputed != record.op_id {
            return Err(OpLogError::IdentityMismatch { claimed: record.op_id, recomputed });
        }
        Ok(Some(record))
    }

    /// Whether `op_id` is already in the log — `put`'s own idempotency
    /// check, exposed so a caller can decide not to recompute or re-parse
    /// a candidate it is about to gate-check.
    pub fn contains(&self, op_id: &OpId) -> bool {
        self.path(op_id).exists()
    }
}
