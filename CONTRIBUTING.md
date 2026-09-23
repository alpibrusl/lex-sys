# Contributing to lex-sys

This is how the compiler itself is worked on. For how to *write*
lex-sys, read [`AGENTS.md`](AGENTS.md). Everything here is a practice the
repository already follows, and each one that can be checked is checked
by the build.

---

## The gate

CI runs these on linux-x86_64 and darwin-aarch64, and a change is not
done until all of them pass locally:

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

`cargo test` includes the conformance suite, which builds and runs real
programs, the documentation link check, and the file budget below.

---

## How a slice is done

1. **The design is written before the code**, in `docs/`, and what it
   claims is measured, not asserted. `docs/README.md` indexes every
   document with its status.
2. **A claim that turns out false is corrected in place**, in the
   document that made it, with the PR that corrected it. It is not
   silently rewritten.
3. **A feature earns its way in when a program asks for it**, counted by
   reading the programs. The bar is two askers (`docs/standard-library.md`).
4. **A test that could not have failed proves nothing.** Anything that
   claims coverage is checked by breaking what it covers and watching it
   fail (`docs/differential.md` §3.1).
5. **Each slice is one PR with a row in `docs/ROADMAP.md`.** The row is
   written after the PR number is known.

---

## Files

**No source file over 2,000 lines.** `crates/lex-sys/tests/files.rs`
enforces it on every `.rs`, `.ls`, `.py` and `.c` file in the
repository. rustc's own `tidy` refuses files over 3,000 lines, and these
crates are smaller. The two files that were already over the budget
when it was introduced have **ceilings**: each may only shrink, its
number comes down as it does, and its row is deleted once it is under
the budget. Raising a ceiling is not a fix.

When a file gets close, split it **by concern**, not by size:

- `lib.rs` is the table of contents. It holds the crate's documentation,
  its `mod` declarations, its public re-exports and its entry points,
  and little else. `lex-sys-ir` is the example: `ir.rs` is the IR,
  `defs.rs` the declarations, `function.rs` one function at a time, and
  `lower/` the walk over a body.
- **A large `impl` can be split across child modules.** Children see
  their parent's private fields, so `lower/stmt.rs`, `lower/memory.rs`
  and `lower/expr.rs` each hold part of `impl FnLowering` without making
  anything more visible.
- **Unit tests live in their own files**, beside the code. They are
  declared with `#[cfg(test)] #[path = "tests/…"] mod …;` so a test's
  name does not change when it moves.
- **A move is a pure move.** Splitting a file changes no behaviour, and
  shows that: the same test names and counts before and after, and
  every line of the old file present in the new ones apart from
  formatting.

---

## Code

- **Visibility is as narrow as it can be.** Crate-internal items are
  `pub(crate)`, and the public API is what `lib.rs` re-exports.
- **Imports are explicit in new code.** The modules split out of
  `lex-sys-ir` start with `use crate::*;` because the split was a pure
  move out of one namespace. New modules name what they use.
- **Every refusal has a rule tag** (`lex_sys_syntax::Rule`), a located
  span where one exists, and a fixture under `tests/reject/` that
  reaches it (`every_rule_has_a_fixture`).
- **Input never reaches a panic.** An `unreachable!` or `expect` states
  an invariant between the checker and the backend, with its reason in
  the message. If one ever fires, it is reported as an `internal`
  refusal at the function it fired in, not as a crash
  (`docs/internal-errors.md`).
- **Comments say why, not what.** Each one cites the document or section
  its decision comes from.

---

## Commits and pull requests

A commit message says what changed and why, in prose. A PR describes
what was measured and what was found, including what is *not* done. CI
must be green on both targets before a merge, and a red run is
root-caused, not re-run until it passes.
