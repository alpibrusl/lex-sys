//! `narrow` into several capabilities (`docs/narrowing-into-several.md`): the
//! per-literal check the single form shares, the pairwise check, and the tuple
//! the multi-literal form answers.

use crate::*;

/// What one literal must satisfy to narrow `current`, the single form and
/// each member of the multi-literal form alike: inside `current`, at a
/// separator for the two path capabilities, and not equal to it.
pub(super) fn check_narrowing(
    which: usize,
    current: &str,
    target: &str,
    span: Span,
) -> Result<(), Diagnostic> {
    if !target.starts_with(current) {
        return Err(Diagnostic::new(
            Rule::CapabilityNotNarrowable,
            format!(
                "`{current}` cannot be narrowed to `{target}`: a capability is attenuated, never widened, and a program must not be able to grant itself what it was not given"
            ),
            span,
        ));
    }
    // A path prefix extends at a separator or not at all. `/tmp` is not
    // a prefix of `/tmpevil` in any sense a filesystem would recognise,
    // and a textual check that said otherwise would hand a program the
    // directory next door. `Ffi` has no separator and no such case, and
    // neither does `Net`: `docs/net.md` §4 bounds a `net_out` label by
    // plain textual prefix on `"host:port"`, the same way an `egress`
    // entry does, with no boundary character of its own.
    // `docs/processes.md` §4.1: `Exec`'s prefix is a path, with `Fs`'s rule.
    if (which == PRELUDE_FS || which == PRELUDE_EXEC || which == PRELUDE_TTY)
        && !extends_path(current, target)
    {
        return Err(Diagnostic::new(
            Rule::CapabilityNotNarrowable,
            format!(
                "`{current}` cannot be narrowed to `{target}`: a path prefix extends at a `/`, and `{target}` is a different name that merely starts with the same bytes"
            ),
            span,
        ));
    }
    if target == current {
        return Err(Diagnostic::new(
            Rule::CapabilityNotNarrowable,
            format!("this narrows `{current}` to itself, which grants nothing new"),
            span,
        ));
    }
    Ok(())
}

/// Why `narrow(cap, "a", "b", ...)` is refused on a capability that is not a
/// path (section 5, item 4): the message names the set form where there is one.
pub(super) fn narrow_many_refusal(which: usize) -> String {
    match which {
        PRELUDE_FFI => "`narrow` into several capabilities is for `Fs` and `Exec`; `Ffi` narrows to a set in one capability: `narrow(ffi, \"libc,libm\")` (`docs/foreign-authority.md` section 5.2)".to_string(),
        PRELUDE_SIGNALS => "`narrow` into several capabilities is for `Fs` and `Exec`; `Signals` narrows to a set in one capability: `narrow(signals, \"TERM,INT\")` (`docs/signals.md` section 2.1)".to_string(),
        _ => "`narrow` into several capabilities is for `Fs` and `Exec`; `Net` is not supported there (`docs/narrowing-into-several.md` section 5, item 4)".to_string(),
    }
}

/// `narrow(cap, "a", "b", ...)`: every literal checked as the single form
/// checks it, no two equal and none inside another, and a tuple of `cap`'s
/// type narrowed to each, in the order written.
///
/// The capability is lowered once, as an owned use (so it is consumed, and the
/// children are the only way on). It is zero-sized, so the tuple's other
/// components are empty tuples: they carry no leaf, and neither backend emits
/// anything for them (`docs/narrowing-into-several.md` section 4.1).
pub(super) fn narrow_into_several(
    value: Expr,
    which: usize,
    current: &str,
    targets: &[String],
    spans: &[Span],
    span: Span,
) -> Result<(Expr, Type), Diagnostic> {
    for (target, at) in targets.iter().zip(spans) {
        check_narrowing(which, current, target, *at)?;
    }
    for (i, a) in targets.iter().enumerate() {
        for (j, b) in targets.iter().enumerate().skip(i + 1) {
            let why = if a == b {
                "are the same path, so the second grants nothing the first does not"
            } else if extends_path(a, b) || extends_path(b, a) {
                "are nested, so the inner one grants nothing the outer one does not"
            } else {
                continue;
            };
            return Err(Diagnostic::new(
                Rule::CapabilityNotNarrowable,
                format!(
                    "`narrow` into several capabilities needs unrelated paths, and `{a}` and `{b}` {why}"
                ),
                spans.get(j).copied().unwrap_or(span),
            ));
        }
    }
    let mut parts = vec![value];
    parts.extend(targets.iter().skip(1).map(|_| Expr::Tuple { parts: Vec::new() }));
    let child = |target: &String| Type::Named(DefId(which as u32), vec![Type::Lit(target.clone())]);
    Ok((Expr::Tuple { parts }, Type::Tuple(targets.iter().map(child).collect())))
}
