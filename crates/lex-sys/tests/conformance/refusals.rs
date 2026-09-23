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
