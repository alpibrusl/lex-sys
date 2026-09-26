//! `docs/vcs.md` §8's second slice: a minimal op log, checked for the one
//! property a content-addressed store actually promises -- what comes
//! back is what was written, named by what it hashes to.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use lex_sys_vcs::{OpLog, OpLogError, Operation, OperationKind, OperationRecord};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A fresh, empty directory this test owns alone -- parallel test threads
/// each get their own counter value, so no two calls collide.
fn scratch_dir() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("lex-sys-vcs-op-log-test-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch dir under the system temp dir");
    dir
}

fn sample_record() -> OperationRecord {
    let mut effects = BTreeSet::new();
    effects.insert("io_write".to_string());
    let op = Operation::new(
        OperationKind::AddFunction {
            sig_id: "a".repeat(64),
            stage_id: "b".repeat(64),
            effects,
            in_file: None,
        },
        1,
        [],
    );
    OperationRecord::new(op)
}

#[test]
fn a_put_record_reads_back_unchanged() {
    let dir = scratch_dir();
    let log = OpLog::open(&dir).expect("open creates the ops/ dir");
    let record = sample_record();

    log.put(&record).expect("put persists a fresh record");
    let back = log.get(&record.op_id).expect("get succeeds").expect("the record is there");
    assert_eq!(back, record);
}

#[test]
fn an_absent_op_id_reads_back_as_none_not_an_error() {
    let dir = scratch_dir();
    let log = OpLog::open(&dir).expect("open creates the ops/ dir");
    assert!(log.get(&"c".repeat(64)).expect("a missing record is not an error").is_none());
}

#[test]
fn writing_the_same_record_twice_is_a_no_op() {
    let dir = scratch_dir();
    let log = OpLog::open(&dir).expect("open creates the ops/ dir");
    let record = sample_record();

    log.put(&record).expect("first put succeeds");
    log.put(&record).expect("second put is idempotent, not an error");
    assert!(log.contains(&record.op_id));
}

#[test]
fn a_record_whose_op_id_disagrees_with_its_own_payload_is_refused() {
    let dir = scratch_dir();
    let log = OpLog::open(&dir).expect("open creates the ops/ dir");
    let mut record = sample_record();
    // Hand-corrupt the claimed identity without touching the payload it
    // was computed from -- the one case `put` cannot see (it always
    // computes `op_id` itself) but a hand-edited or bit-rotted file on
    // disk could produce.
    record.op_id = "d".repeat(64);
    let bytes = serde_json::to_vec(&record).unwrap();
    std::fs::write(dir.join("ops").join(format!("{}.json", record.op_id)), bytes).unwrap();

    match log.get(&record.op_id) {
        Err(OpLogError::IdentityMismatch { claimed, .. }) => assert_eq!(claimed, record.op_id),
        other => panic!("expected IdentityMismatch, got {other:?}"),
    }
}
