//! `examples/seek/` — `docs/agent-tools.md`'s own claim, checked: a bounded
//! authority report, correctness on ordinary input, and no crash on the
//! adversarial input an agent cannot vouch for.

use super::*;

// Each caller passes its own tag: `build_example`'s `scratch()` names a
// directory after it and removes whatever was there first, so two tests
// sharing one tag race each other under `cargo test`'s default
// parallelism -- found the way it usually is, by the suite being flaky
// rather than by reading the helper.
fn build_seek(tag: &str) -> (PathBuf, PathBuf) {
    build_example(&format!("agent-tools-seek-{tag}"), "examples/seek/seek.ls", "seek")
}

fn run_seek(exe: &Path, dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(exe).args(args).current_dir(dir).output().expect("the program runs")
}

/// `docs/agent-tools.md` §2: the whole claim, pinned. Seven labels,
/// `bounded: true` — the same shape `the_report_fails_closed` already
/// pins for `cut`. When this program's authority widens, whoever widened
/// it corrects this test and the document in the same change.
#[test]
fn seek_reports_a_bounded_authority() {
    let (effects, symbols, _) = authority_of("examples/seek/seek.ls");
    assert_eq!(
        effects,
        vec!["args", "err_write", "file_read", "fs_read", "heap", "io_read", "io_write"],
        "`seek`'s report should name exactly these seven labels -- if it reports \
         more, `docs/agent-tools.md` §2's claim is now false and the document \
         needs correcting in the same change"
    );
    assert!(symbols.is_empty(), "`seek` calls no foreign code, so it should list none");

    let out = Command::new(BIN)
        .args([
            "authority".as_ref(),
            repo_root().join("examples/seek/seek.ls").as_os_str(),
            "--std".as_ref(),
            "--output".as_ref(),
            "json".as_ref(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let report = String::from_utf8_lossy(&out.stdout);
    assert!(
        report.trim_start().starts_with("{\n  \"bounded\": true,"),
        "a program with no foreign code is bounded, or the flag means nothing:\n{report}"
    );
}

/// A literal match, printed with its file name prefixed -- the default
/// once more than nothing is named -- and the `0` exit GNU `grep` uses
/// for "at least one match".
#[test]
fn seek_finds_a_literal_match() {
    let (dir, exe) = build_seek("match");
    std::fs::write(dir.join("a.txt"), "hello world\nfoo bar\nhello again\nbaz\n")
        .expect("a writable fixture");

    let out = run_seek(&exe, &dir, &["hello", "a.txt"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a.txt:hello world\na.txt:hello again\n");
    let _ = std::fs::remove_dir_all(&dir);
}

/// No match anywhere is exit `1`, GNU `grep`'s own vocabulary -- not an
/// error, and not confused with one by a caller scripting against it.
#[test]
fn seek_exits_one_on_no_match() {
    let (dir, exe) = build_seek("no-match");
    std::fs::write(dir.join("a.txt"), "hello world\n").expect("a writable fixture");

    let out = run_seek(&exe, &dir, &["xyz", "a.txt"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `-n` prefixes the 1-based line number, matching GNU `grep -n`.
#[test]
fn seek_dash_n_prefixes_the_line_number() {
    let (dir, exe) = build_seek("dash-n");
    std::fs::write(dir.join("a.txt"), "hello world\nfoo bar\nhello again\nbaz\n")
        .expect("a writable fixture");

    let out = run_seek(&exe, &dir, &["-n", "hello", "a.txt"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a.txt:1:hello world\na.txt:3:hello again\n");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `-c` prints a count instead of the matching lines.
#[test]
fn seek_dash_c_prints_a_count() {
    let (dir, exe) = build_seek("dash-c");
    std::fs::write(dir.join("a.txt"), "hello world\nfoo bar\nhello again\nbaz\n")
        .expect("a writable fixture");

    let out = run_seek(&exe, &dir, &["-c", "hello", "a.txt"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a.txt:2\n");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/agent-tools.md` §3.2: `-m` caps matches across every file named,
/// not per file -- the second file's own match never runs because the
/// budget is already spent by the first.
#[test]
fn seek_dash_m_caps_across_files() {
    let (dir, exe) = build_seek("dash-m");
    std::fs::write(dir.join("a.txt"), "hello world\nhello again\n").expect("a writable fixture");
    std::fs::write(dir.join("b.txt"), "hello once more\n").expect("a writable fixture");

    let out = run_seek(&exe, &dir, &["-m", "1", "hello", "a.txt", "b.txt"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "a.txt:hello world\n",
        "the budget should be spent by the first file, leaving nothing for the second"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file that cannot be read is a per-file error, reported and moved
/// past -- GNU `grep`'s own behaviour -- but the overall exit is `2`,
/// not the `0`/`1` a caller would read as "ran fine, no match".
#[test]
fn seek_reports_and_continues_past_a_missing_file() {
    let (dir, exe) = build_seek("missing-file");
    std::fs::write(dir.join("a.txt"), "hello world\n").expect("a writable fixture");

    let out = run_seek(&exe, &dir, &["hello", "missing.txt", "a.txt"]);
    assert_eq!(out.status.code(), Some(2), "a read error anywhere should force exit 2");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "a.txt:hello world\n",
        "a file that failed should not stop the files after it from being searched"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("missing.txt"),
        "the error should name which file could not be read"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// No pattern at all is a usage error, exit `2` -- distinct from "ran and
/// found nothing".
#[test]
fn seek_with_no_pattern_is_a_usage_error() {
    let (dir, exe) = build_seek("usage");
    let out = run_seek(&exe, &dir, &[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&out.stderr).is_empty(), "a usage error should say so");
    let _ = std::fs::remove_dir_all(&dir);
}

/// No file named reads standard input, the same fallback
/// `examples/sort/` already has.
#[test]
fn seek_reads_standard_input_when_no_file_is_named() {
    use std::io::Write as _;

    let (dir, exe) = build_seek("stdin");
    let mut child = Command::new(&exe)
        .arg("hello")
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the program runs");
    child
        .stdin
        .take()
        .expect("a piped stdin")
        .write_all(b"nothing here\nhello stdin\n")
        .expect("the write succeeds");
    let out = child.wait_with_output().expect("the program finishes");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello stdin\n");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/agent-tools.md` §1 and §4: no UB on a file that is not text at
/// all. A NUL byte and a lone `0xFF` sit either side of the pattern, and
/// the search finds it and prints the line whole, byte for byte, rather
/// than crashing, truncating at the NUL, or refusing the file for not
/// being UTF-8 -- none of which this program ever checks for.
#[test]
fn seek_is_binary_safe() {
    let (dir, exe) = build_seek("binary-safe");
    let mut bytes = b"binary".to_vec();
    bytes.push(0);
    bytes.extend_from_slice(b"stuff");
    bytes.push(0xff);
    bytes.extend_from_slice(b"hello");
    bytes.push(b'\n');
    std::fs::write(dir.join("bin.dat"), &bytes).expect("a writable fixture");

    let out = run_seek(&exe, &dir, &["hello", "bin.dat"]);
    assert_eq!(out.status.code(), Some(0), "a match inside binary data should still be found");
    let mut expected = b"bin.dat:".to_vec();
    expected.extend_from_slice(&bytes);
    assert_eq!(out.stdout, expected, "the line should come back whole, byte for byte");
    let _ = std::fs::remove_dir_all(&dir);
}
