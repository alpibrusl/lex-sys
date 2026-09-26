//! The write-time gate: does a candidate program typecheck?
//!
//! `docs/vcs.md` §3 named this as porting from `lex-vcs`'s own `gate.rs`
//! with no code changed at the boundary, because that file already scopes
//! itself narrowly: *"the gate itself does not load anything from disk; it
//! just runs the type checker. Computing that [candidate] sequence is the
//! caller's job."* This file keeps exactly that scope. Assembling *which*
//! source constitutes the candidate — reconstructing a program from an
//! `Operation`'s parents plus its own delta — is a store's job
//! (`docs/vcs.md` §8's next slice after this one), not this file's.

use lex_sys_syntax::{Ast, SourceMap, parse_into};

/// One reason a candidate program did not typecheck: the same `rule`
/// string `lex-sys check --output json`/`lex-sys authority` already
/// report (`docs/agent-errors.md`), so a caller that already knows how to
/// read one of those knows how to read this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateDiagnostic {
    pub rule: String,
    pub message: String,
}

/// Type-check a candidate program assembled by the caller.
///
/// `files` is `(name, source)` pairs, named the way `lex-sys check` names
/// its own inputs. A parse failure ends the check with that one
/// diagnostic, the same rule `compile_reporting` in the CLI follows and
/// for the same reason (`docs/agent-errors.md` §4): a program that did not
/// parse has no reliable second error.
///
/// No `main`-shape check runs here (`compile_reporting`'s own extra rows
/// for `ProgramShape`): an operation can be about a library declaration
/// with no entry point at all, and this function answers "does it
/// typecheck," not "can it be built into an executable."
pub fn check_candidate(files: &[(&str, &str)]) -> Result<(), Vec<GateDiagnostic>> {
    let mut map = SourceMap::new();
    let mut ast = Ast::new();
    for &(name, text) in files {
        let base = map.add(name.to_owned(), text.to_owned());
        if let Err(d) = parse_into(&mut ast, text, base) {
            return Err(vec![GateDiagnostic { rule: d.rule.tag().to_owned(), message: d.message }]);
        }
    }
    match lex_sys_ir::lower_all(&ast) {
        Ok(_program) => Ok(()),
        Err(diagnostics) => Err(diagnostics
            .into_iter()
            .map(|d| GateDiagnostic { rule: d.rule.tag().to_owned(), message: d.message })
            .collect()),
    }
}
