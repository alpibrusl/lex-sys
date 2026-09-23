# CLAUDE.md — lex-sys

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) before changing the compiler,
and [`AGENTS.md`](AGENTS.md) before writing lex-sys programs. The rules
that matter most:

- **The gate:** `cargo fmt --all --check`,
  `cargo clippy --all-targets -- -D warnings` and `cargo test --workspace`,
  all passing, before anything is called done.
- **Design before code**, in `docs/`, with claims measured. A claim that
  turns out false is corrected in place, in the document that made it.
- **No source file over 2,000 lines** (`crates/lex-sys/tests/files.rs`).
  Split by concern; never raise a ceiling.
- **Every refusal has a rule tag**, and no input may reach a panic.
