//! The label-to-imports table (`docs/wasm.md`, W2b) accounts for every label the
//! checker can produce, says only true things about WASI preview 1, and agrees
//! with which builtins a WASI target refuses. Text and tables only, so CI runs it;
//! that the *numbers* are right is `scripts/wasm_authority_check.py`'s job.

use crate::{
    Builtin, EXIT, LABEL_IMPORTS, PREVIEW1_FUNCTIONS, REFUSED_LABELS, UNBOUNDED_LABELS,
    every_label, label_imports, wasi_gap, wasi_imports,
};

#[test]
fn every_label_the_checker_can_produce_is_classified_exactly_once() {
    for label in every_label() {
        let places = [
            label_imports(label).is_some(),
            REFUSED_LABELS.contains(&label),
            UNBOUNDED_LABELS.contains(&label),
        ];
        assert_eq!(
            places.iter().filter(|p| **p).count(),
            1,
            "`{label}` must be supported, refused or unbounded, and only one of them: {places:?}"
        );
    }
    // And the other direction: nothing in the table is a label that does not exist.
    let all = every_label();
    for label in LABEL_IMPORTS
        .iter()
        .map(|r| r.label)
        .chain(REFUSED_LABELS.iter().copied())
        .chain(UNBOUNDED_LABELS.iter().copied())
    {
        assert!(all.contains(&label), "`{label}` is in the table and is not a label");
    }
    // 28 + the three tty labels (`docs/tty.md` §4: `tty_open`
    // argument-carrying, `tty_read`/`tty_write` path-free), all refused
    // on WASI (§9).
    assert_eq!(all.len(), 31, "the checker's labels moved; account for the new one: {all:?}");
}

#[test]
fn what_a_label_requires_is_inside_what_it_allows() {
    for row in LABEL_IMPORTS {
        for needed in row.required {
            assert!(
                row.allowed.contains(needed),
                "`{}` requires `{needed}` but does not allow it",
                row.label
            );
        }
    }
}

#[test]
fn every_import_named_is_a_function_wasi_preview_1_has() {
    assert_eq!(PREVIEW1_FUNCTIONS.len(), 46);
    for name in EXIT.iter().chain(LABEL_IMPORTS.iter().flat_map(|r| r.allowed)) {
        assert!(PREVIEW1_FUNCTIONS.contains(name), "`{name}` is not a WASI preview 1 function");
    }
}

#[test]
fn the_table_is_sorted_and_has_no_duplicates() {
    let labels: Vec<&str> = LABEL_IMPORTS.iter().map(|r| r.label).collect();
    let unique: std::collections::BTreeSet<&str> = labels.iter().copied().collect();
    assert_eq!(unique.len(), labels.len(), "a label appears twice");
    for row in LABEL_IMPORTS {
        for set in [row.required, row.allowed] {
            let mut sorted = set.to_vec();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted, set, "`{}`'s imports are not sorted and unique", row.label);
        }
    }
}

#[test]
fn a_row_adds_up_what_its_labels_license() {
    let got = wasi_imports(["heap", "io_read", "io_write", "err_write"]);
    assert_eq!(got.required, ["fd_read", "fd_write"]);
    assert_eq!(got.allowed, ["fd_read", "fd_write", "proc_exit"], "proc_exit is always allowed");
    assert!(got.refused.is_empty() && got.unbounded.is_empty() && got.unknown.is_empty());

    // A program with an empty row requires nothing, and may import only how it exits: its
    // module's import section is empty.
    let pure = wasi_imports([]);
    assert!(pure.required.is_empty());
    assert_eq!(pure.allowed, EXIT);

    // `args` is a label like any other now: the command line is fetched only for a program
    // that reads it.
    let args = wasi_imports(["args"]);
    assert_eq!(args.required, ["args_get", "args_sizes_get"]);

    // A label is coarser than a builtin: `dir_write` allows rename and remove, and
    // requires none of them, because a program with that label may only create.
    let dirs = wasi_imports(["dir_write"]);
    assert!(dirs.allowed.contains(&"path_rename") && dirs.required.is_empty());
    assert!(!dirs.required.contains(&"proc_exit"), "proc_exit is never required");

    // Refused and unbounded labels are reported, not silently licensed.
    let mixed = wasi_imports(["io_write", "conc", "net_out", "ffi", "no_such_label"]);
    assert_eq!(mixed.refused, ["conc", "net_out"]);
    assert_eq!(mixed.unbounded, ["ffi"]);
    assert_eq!(mixed.unknown, ["no_such_label"]);
    assert_eq!(mixed.allowed, ["fd_write", "proc_exit"]);
}

#[test]
fn a_builtin_wasi_supports_never_carries_a_label_it_refuses() {
    // `wasi_gap` (what `check --target` refuses) and the label table (what a module
    // may import) are two views of the same line; this keeps them one line.
    for builtin in Builtin::ALL {
        if wasi_gap(*builtin).is_some() {
            continue;
        }
        for label in builtin.effects().labels() {
            assert!(
                !REFUSED_LABELS.contains(&label.name.as_str()),
                "`{}` is supported on WASI but performs `{}`, which the table refuses",
                builtin.name(),
                label.name
            );
        }
    }
}
