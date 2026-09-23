//! Repository hygiene: no source file outgrows the budget
//! (`CONTRIBUTING.md`, "Files").
//!
//! `lex-sys-ir/src/lib.rs` reached 10,060 lines one slice at a time, and
//! nothing said no. This is what says no.

use std::path::{Path, PathBuf};

/// The most lines a source file may have.
///
/// rustc's own `tidy` refuses a file over 3,000 lines. These crates are
/// smaller, and a file that one person can read in a sitting is the
/// point, so the bar is 2,000.
const BUDGET: usize = 2_000;

/// Files over the budget, each held to its size: a ceiling may only come
/// down. Split the file, lower the number, and delete the row once the
/// file is under [`BUDGET`]. Raising one is not a fix.
///
/// Empty. The two files over the budget when it was introduced,
/// `lex-sys-codegen/src/lib.rs` (2,886) and `tests/conformance.rs` (6,684),
/// were split right after it. The table stays, for the day the budget is
/// tightened and the files over the new line need somewhere to wait.
const CEILINGS: &[(&str, usize)] = &[];

const SOURCE_EXTENSIONS: &[&str] = &["rs", "ls", "py", "c"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("the workspace root is two levels above this crate")
}

fn sources(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("a readable directory") {
            let path = entry.expect("a readable entry").path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if path.is_dir() {
                if name != "target" && !name.starts_with('.') {
                    stack.push(path);
                }
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| SOURCE_EXTENSIONS.contains(&e))
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

#[test]
fn no_source_file_outgrows_the_budget() {
    let root = repo_root();
    let mut over = Vec::new();
    let mut seen_ceilings = Vec::new();
    for path in sources(&root) {
        let relative =
            path.strip_prefix(&root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        let lines = std::fs::read_to_string(&path).expect("a readable source file").lines().count();
        match CEILINGS.iter().find(|(file, _)| *file == relative) {
            Some(&(_, ceiling)) => {
                seen_ceilings.push(relative.clone());
                if lines > ceiling {
                    over.push(format!(
                        "{relative}: {lines} lines, over its ceiling of {ceiling}. Split it rather than raising the number"
                    ));
                } else if lines <= BUDGET {
                    over.push(format!(
                        "{relative}: {lines} lines, under the budget now. Delete its row from CEILINGS"
                    ));
                } else if lines < ceiling {
                    over.push(format!(
                        "{relative}: {lines} lines, below its ceiling of {ceiling}. Lower the ceiling to {lines} so it cannot grow back"
                    ));
                }
            }
            None if lines > BUDGET => over.push(format!(
                "{relative}: {lines} lines, over the {BUDGET}-line budget. Split it by concern (CONTRIBUTING.md, \"Files\")"
            )),
            None => {}
        }
    }
    for (file, _) in CEILINGS {
        assert!(
            seen_ceilings.iter().any(|seen| seen == file),
            "`{file}` has a ceiling and no longer exists. Delete its row"
        );
    }
    assert!(over.is_empty(), "{}", over.join("\n"));
}
