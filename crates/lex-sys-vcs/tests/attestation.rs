//! `docs/vcs.md` §3/§8: a hash-chained attestation log, checked for
//! exactly what `lex-os-audit`'s own doc comment says a bare chain
//! catches and does not.

use lex_sys_vcs::{AttestationEvent, Chain, GENESIS, SigningKey, VerifyingKey};

fn accepted(op_id: &str) -> AttestationEvent {
    AttestationEvent::GateAccepted { op_id: op_id.to_string() }
}

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

#[test]
fn an_empty_chain_heads_at_genesis_and_verifies() {
    let chain: Chain<AttestationEvent> = Chain::new();
    assert_eq!(chain.head(), GENESIS);
    assert_eq!(chain.verify(), Ok(()));
}

#[test]
fn appended_entries_chain_from_each_others_hash() {
    let mut chain = Chain::new();
    chain.append(accepted(&"a".repeat(64)));
    chain.append(accepted(&"b".repeat(64)));
    chain.append(accepted(&"c".repeat(64)));

    assert_eq!(chain.len(), 3);
    assert_eq!(chain.verify(), Ok(()));

    let entries = chain.entries();
    assert_eq!(entries[0].prev_hash, GENESIS);
    assert_eq!(entries[1].prev_hash, entries[0].hash);
    assert_eq!(entries[2].prev_hash, entries[1].hash);
    assert_eq!(chain.head(), entries[2].hash);
}

#[test]
fn editing_a_payload_after_the_fact_breaks_verification() {
    let mut chain = Chain::new();
    chain.append(accepted(&"a".repeat(64)));
    chain.append(accepted(&"b".repeat(64)));

    // Round-trip through JSON, corrupt one entry's payload in place (the
    // hash is not recomputed), and load it back -- the tamper a holder
    // who does not also recompute hashes would actually make.
    let mut json: serde_json::Value = serde_json::to_value(&chain).unwrap();
    json["entries"][0]["event"]["op_id"] = serde_json::Value::String("f".repeat(64));
    let tampered: Chain<AttestationEvent> = serde_json::from_value(json).unwrap();

    let err = tampered.verify().expect_err("an edited payload must not verify");
    assert_eq!(err.seq, 0);
}

#[test]
fn reordering_entries_breaks_verification() {
    let mut chain = Chain::new();
    chain.append(accepted(&"a".repeat(64)));
    chain.append(accepted(&"b".repeat(64)));

    let mut json: serde_json::Value = serde_json::to_value(&chain).unwrap();
    let entries = json["entries"].as_array_mut().unwrap();
    entries.swap(0, 1);
    let tampered: Chain<AttestationEvent> = serde_json::from_value(json).unwrap();

    assert!(tampered.verify().is_err());
}

#[test]
fn truncating_the_tail_still_verifies_and_that_is_the_documented_limit() {
    // `attestation.rs`'s own module doc, carried straight from
    // `lex-os-audit`: a prefix of a valid chain is itself a valid chain.
    // This is not a bug to fix here -- it is the reason `lex-os-audit`
    // has checkpoints, which this file deliberately does not build yet.
    let mut chain = Chain::new();
    chain.append(accepted(&"a".repeat(64)));
    chain.append(accepted(&"b".repeat(64)));
    chain.append(accepted(&"c".repeat(64)));

    let mut json: serde_json::Value = serde_json::to_value(&chain).unwrap();
    json["entries"].as_array_mut().unwrap().truncate(2);
    let truncated: Chain<AttestationEvent> = serde_json::from_value(json).unwrap();

    assert_eq!(truncated.len(), 2);
    assert_eq!(truncated.verify(), Ok(()));
}

#[test]
fn an_unsealed_chain_appends_no_seal_at_all() {
    let mut chain: Chain<AttestationEvent> = Chain::new();
    assert!(!chain.is_sealing());
    chain.append(accepted(&"a".repeat(64)));
    assert!(chain.entries()[0].seal.is_none());
}

#[test]
fn a_sealed_chain_seals_every_entry_it_appends() {
    let signing_key = key(7);
    let mut chain: Chain<AttestationEvent> = Chain::new().sealed_with(signing_key.clone());
    assert!(chain.is_sealing());
    chain.append(accepted(&"a".repeat(64)));
    chain.append(accepted(&"b".repeat(64)));

    assert_eq!(chain.verify(), Ok(()));
    assert_eq!(chain.verify_seals(&signing_key.verifying_key()), Ok(()));
    for entry in chain.entries() {
        let seal = entry.seal.as_ref().expect("every entry was appended under a sealing key");
        assert_eq!(seal.signer, hex::encode(signing_key.verifying_key().to_bytes()));
    }
}

#[test]
fn verify_seals_rejects_the_wrong_key() {
    let mut chain: Chain<AttestationEvent> = Chain::new().sealed_with(key(7));
    chain.append(accepted(&"a".repeat(64)));

    let wrong: VerifyingKey = key(9).verifying_key();
    assert!(chain.verify_seals(&wrong).is_err());
}

#[test]
fn verify_seals_rejects_an_entry_with_no_seal_at_all() {
    // A chain that started unsealed and was later re-opened with
    // `sealed_with` -- exactly the case `sealed_with`'s own doc comment
    // names: entries already written are left alone, on purpose, so an
    // entry from before sealing was turned on has no seal to check.
    let mut chain: Chain<AttestationEvent> = Chain::new();
    chain.append(accepted(&"a".repeat(64)));
    let sealed_key = key(7);
    chain = chain.sealed_with(sealed_key.clone());
    chain.append(accepted(&"b".repeat(64)));

    let err = chain
        .verify_seals(&sealed_key.verifying_key())
        .expect_err("the first entry predates the key and has no seal");
    assert_eq!(err.seq, 0);
}

#[test]
fn a_seal_does_not_change_the_entrys_own_hash() {
    // Sealing is deliberately not part of `compute_hash` -- the same
    // payload sealed and unsealed must chain identically, so turning
    // sealing on partway through a log does not retroactively change
    // anything already written.
    let mut unsealed: Chain<AttestationEvent> = Chain::new();
    unsealed.append(accepted(&"a".repeat(64)));

    let mut sealed: Chain<AttestationEvent> = Chain::new().sealed_with(key(7));
    sealed.append(accepted(&"a".repeat(64)));

    assert_eq!(unsealed.entries()[0].hash, sealed.entries()[0].hash);
}

#[test]
fn a_gate_rejection_records_the_rules_that_refused_it() {
    let mut chain = Chain::new();
    chain.append(AttestationEvent::GateRejected {
        op_id: "a".repeat(64),
        rules: vec!["effect-not-declared".to_string()],
    });
    assert_eq!(chain.verify(), Ok(()));
    match &chain.entries()[0].event {
        AttestationEvent::GateRejected { rules, .. } => {
            assert_eq!(rules, &vec!["effect-not-declared".to_string()])
        }
        other => panic!("expected GateRejected, got {other:?}"),
    }
}
