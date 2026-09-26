//! `docs/vcs.md` §3's claim that the apply→gate pipeline needs no change
//! at the boundary, checked directly: a candidate that typechecks passes,
//! one that does not is refused with the same `rule` tag `lex-sys check`
//! would report.

use lex_sys_vcs::check_candidate;

#[test]
fn a_candidate_that_typechecks_is_accepted() {
    let files = [("f.ls", "fn f(a: int, b: int) -> [] int { return a + b; }")];
    assert_eq!(check_candidate(&files), Ok(()));
}

#[test]
fn a_candidate_that_does_not_typecheck_is_refused_with_its_rule() {
    // `tests/reject/io_read_undeclared.ls`'s own shape: a helper that
    // holds an `Io` and reads from it while its row claims `[]` --
    // `EffectNotDeclared` in `docs/agent-errors.md`'s own catalogue.
    let files = [("f.ls", "fn quiet[&i](io: &!i Io) -> [] int { return getchar(io); }")];
    let errs = check_candidate(&files).expect_err("an undeclared io_read effect is refused");
    assert!(
        errs.iter().any(|e| e.rule == "effect-not-declared"),
        "expected an effect-not-declared refusal, got {errs:?}"
    );
}

#[test]
fn a_candidate_that_does_not_parse_is_refused_too() {
    let files = [("f.ls", "fn f(")];
    assert!(check_candidate(&files).is_err());
}

#[test]
fn a_library_declaration_with_no_main_still_typechecks() {
    // The one thing this gate deliberately does not enforce
    // (`compile_reporting`'s own `ProgramShape` rows are a CLI-build
    // concern, not a typechecking one): an operation about a library
    // function has no entry point at all.
    let files = [("lib.ls", "pub fn double(n: int) -> [] int { return n * 2; }")];
    assert_eq!(check_candidate(&files), Ok(()));
}
