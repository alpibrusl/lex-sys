//! Refusals as data: `check --output json` (`docs/agent-errors.md`).

use super::*;

/// `docs/agent-errors.md` §4: every independent refusal, not the first.
///
/// Three ill-typed functions in three independent bodies. Before this,
/// one was reported and the other two cost a compile, a read and a turn
/// each — which is almost free for a person with the file open and is
/// the whole cost for a program that has to ask again.
#[test]
fn every_independent_refusal_is_reported() {
    let dir = scratch("batched-refusals");
    let source = dir.join("three.ls");
    std::fs::write(
        &source,
        "fn a() -> [] int { return true; }\n\
         fn b() -> [] int { return oops; }\n\
         fn c() -> [] bool { return 1; }\n\
         fn main() -> [] int { return 0; }\n",
    )
    .expect("the fixture is written");

    let prose = Command::new(BIN).arg("check").arg(&source).output().expect("the compiler runs");
    let text = String::from_utf8_lossy(&prose.stderr);
    assert_eq!(
        text.matches("error:").count(),
        3,
        "three independent bodies should give three refusals:\n{text}"
    );
    // In source order, which is the order checking already runs in.
    let first = text.find("found `bool`").expect("a's refusal");
    let second = text.find("`oops` is not bound").expect("b's refusal");
    let third = text.find("found `int`").expect("c's refusal");
    assert!(first < second && second < third, "refusals are out of source order:\n{text}");

    // And the machine-readable form agrees, object for object.
    let json = Command::new(BIN)
        .arg("check")
        .arg(&source)
        .args(["--output", "json"])
        .output()
        .expect("the compiler runs");
    let body = String::from_utf8_lossy(&json.stdout);
    assert_eq!(body.matches("\"rule\":").count(), 3, "the JSON lost a refusal:\n{body}");
    assert_eq!(json.status.code(), Some(1), "a refused program still exits 1");

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/agent-errors.md` §6: the JSON adds fields beside the prose and
/// rewrites none of it.
///
/// The sentence a person reads is the sentence the object carries, so a
/// consumer of one is never reading something the other does not say —
/// and nobody has to keep two wordings in step.
#[test]
fn the_prose_is_unchanged_by_the_json() {
    for path in fixtures("reject") {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let prose = Command::new(BIN).arg("check").arg(&path).output().expect("the compiler runs");
        let prose = String::from_utf8_lossy(&prose.stderr).into_owned();
        let json = Command::new(BIN)
            .arg("check")
            .arg(&path)
            .args(["--output", "json"])
            .output()
            .expect("the compiler runs");
        let body = String::from_utf8_lossy(&json.stdout);
        let message = body
            .lines()
            .find_map(|line| line.trim().strip_prefix("\"message\": \""))
            .map(|rest| rest.trim_end_matches("\","))
            .unwrap_or_else(|| panic!("`{name}` reported no message:\n{body}"));
        // The JSON escapes what JSON must; compare what survives that.
        let message = message.replace("\\\"", "\"").replace("\\\\", "\\");
        assert!(
            prose.contains(&message),
            "`{name}` says one thing as prose and another as data:\n{prose}\n{message}"
        );
    }
}

/// `docs/agent-errors.md` §5, read by a JSON parser rather than by eye:
/// every reject fixture's answer is valid JSON, and every refusal has a
/// position except §1.1's three about the program as a whole.
///
/// `the_prose_is_unchanged_by_the_json` compared one line of each message
/// and so passed while every parse error answered invalid JSON: the
/// message carried the rendered excerpt with its newlines, and its
/// position was `null`.
#[test]
fn every_refusal_is_valid_json_with_a_position() {
    let program_level = ["main_must_return_int", "main_takes_the_world", "no_main"];
    for path in fixtures("reject") {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let out = Command::new(BIN)
            .arg("check")
            .arg(&path)
            .args(["--output", "json"])
            .output()
            .expect("the compiler runs");
        let body = String::from_utf8_lossy(&out.stdout);
        if let Err(at) = strict_json(&body) {
            panic!("`{name}` answered invalid JSON at byte {at}:\n{body}");
        }
        let unplaced = body.matches("\"position\": null").count();
        if program_level.contains(&name.as_str()) {
            assert_eq!(unplaced, 1, "`{name}` is about the program, not a span:\n{body}");
        } else {
            assert_eq!(unplaced, 0, "`{name}` has a refusal with no position:\n{body}");
        }
    }
}

/// Enough of RFC 8259 to refuse what a JSON parser refuses, and in
/// particular a raw control character inside a string. The error is the
/// byte offset where reading failed.
fn strict_json(text: &str) -> Result<(), usize> {
    fn ws(b: &[u8], mut i: usize) -> usize {
        while i < b.len() && matches!(b[i], b' ' | b'\n' | b'\r' | b'\t') {
            i += 1;
        }
        i
    }
    fn value(b: &[u8], i: usize) -> Result<usize, usize> {
        let i = ws(b, i);
        match b.get(i) {
            Some(b'{') => {
                let mut i = ws(b, i + 1);
                if b.get(i) == Some(&b'}') {
                    return Ok(i + 1);
                }
                loop {
                    i = string(b, ws(b, i))?;
                    i = ws(b, i);
                    if b.get(i) != Some(&b':') {
                        return Err(i);
                    }
                    i = ws(b, value(b, i + 1)?);
                    match b.get(i) {
                        Some(b',') => i += 1,
                        Some(b'}') => return Ok(i + 1),
                        _ => return Err(i),
                    }
                }
            }
            Some(b'[') => {
                let mut i = ws(b, i + 1);
                if b.get(i) == Some(&b']') {
                    return Ok(i + 1);
                }
                loop {
                    i = ws(b, value(b, i)?);
                    match b.get(i) {
                        Some(b',') => i += 1,
                        Some(b']') => return Ok(i + 1),
                        _ => return Err(i),
                    }
                }
            }
            Some(b'"') => string(b, i),
            Some(c) if c.is_ascii_digit() || *c == b'-' => {
                let mut j = i + 1;
                while j < b.len() && (b[j].is_ascii_digit() || b".eE+-".contains(&b[j])) {
                    j += 1;
                }
                Ok(j)
            }
            _ => {
                for word in [&b"null"[..], b"true", b"false"] {
                    if b[i..].starts_with(word) {
                        return Ok(i + word.len());
                    }
                }
                Err(i)
            }
        }
    }
    fn string(b: &[u8], i: usize) -> Result<usize, usize> {
        if b.get(i) != Some(&b'"') {
            return Err(i);
        }
        let mut i = i + 1;
        while let Some(&c) = b.get(i) {
            match c {
                b'"' => return Ok(i + 1),
                b'\\' => match b.get(i + 1) {
                    Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => i += 2,
                    Some(b'u')
                        if b.len() >= i + 6
                            && b[i + 2..i + 6].iter().all(u8::is_ascii_hexdigit) =>
                    {
                        i += 6
                    }
                    _ => return Err(i),
                },
                c if c < 0x20 => return Err(i),
                _ => i += 1,
            }
        }
        Err(i)
    }
    let b = text.as_bytes();
    let end = ws(b, value(b, 0)?);
    if end == b.len() { Ok(()) } else { Err(end) }
}

/// A program with nothing wrong answers an empty list and exits 0.
///
/// The shape a consumer checks the length of, rather than an absence it
/// has to special-case.
#[test]
fn a_clean_program_answers_an_empty_list() {
    let out = Command::new(BIN)
        .arg("check")
        .arg(repo_root().join("examples/hello.ls"))
        .args(["--output", "json"])
        .output()
        .expect("the compiler runs");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "{\n  \"refused\": []\n}\n");
}
