//! `docs/vcs.md` §8's own acceptance for this slice: an `AddFunction`/
//! `ModifyBody` op built from a real lex-sys program's *actual*
//! `lex-sys-id` hashes, not invented strings — checked by hand against
//! `lex-sys ids`' own output before anything is built on top of this.

use std::collections::BTreeSet;

use lex_sys_id::identify;
use lex_sys_syntax::parse;
use lex_sys_vcs::{Operation, OperationKind};

const SOURCE: &str = "fn f(a: int, b: int) -> [] int { return a + b; }";

fn sig_and_stage(source: &str, name: &str) -> (String, String) {
    let ast = parse(source).expect("this fixture parses");
    let identities = identify(&ast);
    let f = identities.function(name).expect("the fixture declares this function");
    (f.sig.to_hex(), f.body.to_hex())
}

#[test]
fn an_add_function_op_hashes_the_real_lex_sys_id_hashes() {
    let (sig_id, stage_id) = sig_and_stage(SOURCE, "f");

    // Checked directly against `lex-sys ids /tmp/sig_check.ls`'s own CLI
    // output on this exact source: `bd30e7a7...753b953` (sig) and
    // `99c546ac...d181090` (body) -- `sig_and_stage` above calls the same
    // `lex_sys_id::identify` the CLI does, so this pins that the two paths
    // agree rather than assuming it.
    assert_eq!(sig_id, "bd30e7a7635f80351dd2f8775f9ca5d157e78273ea78a42221c288347753b953");
    assert_eq!(stage_id, "99c546ac8ea7f0467d1130f959c8bc39eda2f96c37c885223e2f4e619d181090");

    let mut effects = BTreeSet::new();
    effects.insert("io_write".to_string());

    let op = Operation::new(
        OperationKind::AddFunction {
            sig_id: sig_id.clone(),
            stage_id: stage_id.clone(),
            effects,
            in_file: None,
        },
        1,
        [],
    );

    // The one property an `OpId` exists for: same payload, same identity,
    // computed twice independently rather than compared to itself.
    let again = Operation::new(
        OperationKind::AddFunction {
            sig_id,
            stage_id,
            effects: BTreeSet::from(["io_write".to_string()]),
            in_file: None,
        },
        1,
        [],
    );
    assert_eq!(op.op_id(), again.op_id());
}

#[test]
fn parent_order_does_not_change_the_op_id() {
    let (sig_id, stage_id) = sig_and_stage(SOURCE, "f");
    let kind = OperationKind::ModifyBody {
        sig_id,
        from_stage_id: stage_id.clone(),
        to_stage_id: stage_id,
    };

    let forwards = Operation::new(kind.clone(), 1, ["a".to_string(), "b".to_string()]);
    let backwards = Operation::new(kind, 1, ["b".to_string(), "a".to_string()]);

    assert_eq!(forwards.op_id(), backwards.op_id());
}

#[test]
fn a_different_edition_is_a_different_operation() {
    let (sig_id, stage_id) = sig_and_stage(SOURCE, "f");
    let kind =
        OperationKind::AddFunction { sig_id, stage_id, effects: BTreeSet::new(), in_file: None };

    let edition_1 = Operation::new(kind.clone(), 1, []);
    let edition_2 = Operation::new(kind, 2, []);

    assert_ne!(
        edition_1.op_id(),
        edition_2.op_id(),
        "docs/vcs.md §6: the edition is part of what gets hashed, so a vocabulary \
         change moving the effect row this op names is visible in its own identity"
    );
}

#[test]
fn two_functions_with_different_bodies_get_different_stage_ids() {
    let (sig_a, stage_a) = sig_and_stage("fn f(a: int, b: int) -> [] int { return a + b; }", "f");
    let (sig_b, stage_b) = sig_and_stage("fn f(a: int, b: int) -> [] int { return a - b; }", "f");

    // Same signature (same parameter and return types), different body.
    assert_eq!(sig_a, sig_b, "the signature does not depend on the body");
    assert_ne!(stage_a, stage_b, "the body hash is exactly what changed");

    let modify = Operation::new(
        OperationKind::ModifyBody { sig_id: sig_a, from_stage_id: stage_a, to_stage_id: stage_b },
        1,
        [],
    );
    // No assertion beyond "this constructs and hashes" -- the interesting
    // claim is the one above, that the two `StageId`s actually differ.
    let _ = modify.op_id();
}
