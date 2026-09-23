//! The documentation is checked: links resolve, the README's commands work, `AGENTS.md` compiles.

use super::*;

/// `docs/first-page.md` §5 — every local documentation link resolves.
///
/// This ran by hand at the end of every slice until now, which is the
/// wrong place for it: a link that rots between one slice and the next
/// is found by whoever clicks it rather than by the build. 150-odd
/// links across `README.md`, `AGENTS.md` and `docs/` is more than a
/// reader should be asked to trust.
///
/// Fenced blocks and inline code are stripped first, because
/// `alloc_slice[a](5, 0)` is `](` followed by a path that is not one.
#[test]
fn every_documentation_link_resolves() {
    let root = repo_root();
    let mut files: Vec<PathBuf> = vec![root.join("README.md"), root.join("AGENTS.md")];
    for entry in std::fs::read_dir(root.join("docs")).expect("docs/ is readable") {
        let path = entry.expect("a readable entry").path();
        if path.extension().is_some_and(|e| e == "md") {
            files.push(path);
        }
    }
    files.sort();

    let mut checked = 0;
    let mut broken: Vec<String> = Vec::new();
    for file in &files {
        let source = std::fs::read_to_string(file).expect("a readable document");
        // Strip fenced blocks, then inline code, then look for `](target)`.
        let mut text = String::new();
        let mut fenced = false;
        for line in source.lines() {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                continue;
            }
            if !fenced {
                text.push_str(line);
                text.push('\n');
            }
        }
        let mut plain = String::new();
        let mut in_code = false;
        for c in text.chars() {
            if c == '`' {
                in_code = !in_code;
                continue;
            }
            if !in_code {
                plain.push(c);
            }
        }

        let directory = file.parent().expect("a parent directory");
        let mut rest = plain.as_str();
        while let Some(at) = rest.find("](") {
            rest = &rest[at + 2..];
            let Some(close) = rest.find(')') else { break };
            let target = &rest[..close];
            rest = &rest[close + 1..];
            if target.starts_with("http") || target.starts_with("mailto") || target.starts_with('#')
            {
                continue;
            }
            // A `#section` suffix names a heading, not a file.
            let path = target.split('#').next().unwrap_or(target);
            if path.is_empty() || path.contains(char::is_whitespace) {
                continue;
            }
            checked += 1;
            if !directory.join(path).exists() {
                broken.push(format!("{} -> {target}", file.display()));
            }
        }
    }

    assert!(checked > 100, "only {checked} links found; the scan is broken, not the links");
    assert!(
        broken.is_empty(),
        "{} broken documentation link(s):\n{}",
        broken.len(),
        broken.join("\n")
    );
}

/// `docs/first-page.md` §5 — the commands the first page shows still exist.
///
/// `AGENTS.md`'s code blocks are fixtures, for the reason §1 there gives:
/// a page that teaches something the compiler no longer does is worse
/// than no page. The same argument reaches the README, whose
/// "compiler's whole surface" block is the first thing a reader tries.
///
/// The check is against `--help`, not against running each one, because
/// what rots here is a subcommand that was renamed or added — which is
/// exactly what this caught when it was written: `layout` and
/// `agent-guidelines` were missing from the block, and `check` had lost
/// its `--output json`.
#[test]
fn the_readme_commands_still_work() {
    let readme = std::fs::read_to_string(repo_root().join("README.md")).expect("a readable README");
    let help = Command::new(BIN).arg("--help").output().expect("the compiler runs");
    let help = String::from_utf8_lossy(&help.stdout).into_owned();

    // Every `lex-sys <word>` the README shows in its surface block.
    let block = readme
        .split("### The compiler's whole surface")
        .nth(1)
        .expect("the README should still document the surface");
    let shown: Vec<&str> = block
        .lines()
        .take_while(|line| !line.starts_with("###"))
        .filter_map(|line| line.trim().strip_prefix("lex-sys "))
        .filter_map(|rest| rest.split_whitespace().next())
        .filter(|word| !word.starts_with('-'))
        .collect();
    assert!(shown.len() >= 6, "the surface block should list the subcommands, found {shown:?}");

    for command in &shown {
        assert!(
            help.contains(&format!("lex-sys {command}")),
            "the README shows `lex-sys {command}` and `--help` does not:\n{help}"
        );
    }

    // And the other way: a subcommand the compiler has and the page does
    // not is the half that goes unnoticed, because nothing breaks.
    //
    // Scoped to the `usage:` block, because `--help` opens with a prose
    // line that also begins "lex-sys" and names no subcommand.
    let usage = help
        .split("usage:")
        .nth(1)
        .expect("`--help` should have a usage block")
        .split("\noptions:")
        .next()
        .expect("a usage block");
    for line in usage.lines() {
        let Some(rest) = line.trim().strip_prefix("lex-sys ") else { continue };
        let Some(word) = rest.split_whitespace().next() else { continue };
        if word.starts_with('-') {
            continue;
        }
        assert!(
            shown.contains(&word),
            "`lex-sys {word}` exists and the README's surface block does not show it"
        );
    }
}

/// `AGENTS.md`'s checked code blocks are fixtures.
///
/// The document is a contract with whoever writes lex-sys next, and a
/// contract nothing enforces is a stale paragraph. So every block
/// marked `lex-sys` must compile, and every block marked
/// `lex-sys-refused` must be refused **with the rule it names** — the
/// same `//~ RULE` line the fixtures under `tests/reject/` carry
/// (`docs/agent-errors.md` §3).
///
/// Untagged blocks are shell, signatures or tables, and are not run.
/// A block that makes a claim about the language is tagged.
#[test]
fn the_agent_guidelines_compile_as_written() {
    let text =
        std::fs::read_to_string(repo_root().join("AGENTS.md")).expect("AGENTS.md is readable");
    let dir = scratch("agent-guidelines");

    let mut checked = 0;
    let mut rest = text.as_str();
    while let Some(start) = rest.find("```lex-sys") {
        let after = &rest[start + 3..];
        let (kind, body_start) = match after.strip_prefix("lex-sys-refused\n") {
            Some(body) => ("refused", body),
            None => match after.strip_prefix("lex-sys\n") {
                Some(body) => ("accepted", body),
                // `lex-sys` as the start of some other word: skip it.
                None => {
                    rest = &rest[start + 3..];
                    continue;
                }
            },
        };
        let end = body_start.find("```").expect("an unterminated code block in AGENTS.md");
        let body = &body_start[..end];
        rest = &body_start[end..];

        let path = dir.join(format!("block{checked}.ls"));
        std::fs::write(&path, body).expect("the block is written");
        let out = Command::new(BIN)
            .arg("check")
            .arg(&path)
            .args(["--std", "--output", "json"])
            .output()
            .expect("the compiler runs");
        let json = String::from_utf8_lossy(&out.stdout);
        let reported = json
            .lines()
            .find_map(|line| line.trim().strip_prefix("\"rule\": \""))
            .map(|r| r.trim_end_matches("\","));

        match kind {
            "accepted" => assert!(
                out.status.success(),
                "AGENTS.md block {checked} is shown as valid lex-sys and is not:\n{body}\n{json}"
            ),
            _ => {
                let declared = body
                    .lines()
                    .find_map(|line| line.trim().strip_prefix("//~ RULE "))
                    .map(str::trim)
                    .unwrap_or_else(|| {
                        panic!("AGENTS.md block {checked} is shown as refused and names no rule")
                    });
                assert_eq!(
                    reported,
                    Some(declared),
                    "AGENTS.md block {checked} names `{declared}`:\n{body}\n{json}"
                );
            }
        }
        checked += 1;
    }

    // A document with no checked blocks would pass this test while
    // saying nothing, which is the failure mode it exists to prevent.
    assert!(checked >= 5, "only {checked} checked blocks in AGENTS.md");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The compiler carries the guidelines, so a reader with the binary
/// needs no checkout.
#[test]
fn agent_guidelines_prints_the_file() {
    let out = Command::new(BIN).arg("agent-guidelines").output().expect("the compiler runs");
    assert!(out.status.success());
    let printed = String::from_utf8_lossy(&out.stdout);
    let onedisk = std::fs::read_to_string(repo_root().join("AGENTS.md")).expect("readable");
    assert_eq!(printed, onedisk, "the embedded guidelines have drifted from the file");
}
