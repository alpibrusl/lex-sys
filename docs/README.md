# Design documents

Design lands here before the code that implements it. M0 is built
(`docs/bootstrap.md` records what it settled); everything below M2 is still
paper, which is the cheap place for it to be wrong.

| Doc | Purpose | Status |
|---|---|---|
| `linearity-and-effects.md` | The core type-system rules: linear/affine ownership, capability-typed effects, how they unify, and the cases that **must** be rejected. Worked examples throughout. | **written** ([#2](https://github.com/alpibrusl/lex-sys/issues/2)) — awaiting review |
| `bootstrap.md` | What M0 settled to exist: bootstrap host language, file extension, layout, the M0 surface, what is scaffolding and what replaces it. | written ([#3](https://github.com/alpibrusl/lex-sys/issues/3)) |
| `canonical-ast.md` | AST shape, canonicalisation rules, per-unit identity (signature vs body hashing), and the determinism invariants. | **written** — implemented by `lex-sys-id`; `lex-sys ids <file>` prints them. §8 lists what is not yet a contract |
| `memory-model.md` | Regions/arenas, what escapes, the escape hatches (refcount / generational refs) and their runtime cost. | not written — needed for M2, and §5, §6 and §9 of `linearity-and-effects.md` decide most of it |
| `defined-behaviour.md` | Every place C/Rust leave behaviour open, and what we define it to instead. Integer overflow, evaluation order, layout. | not written — needed for M3. Division already traps rather than being undefined, with a fixture to prove it |

`linearity-and-effects.md` is the gating artifact: it is the decision set that
determines whether this is a three-month prototype or a three-year project, and
nothing in M2 should start before it is settled. It is written; it is not
*settled* until it has been reviewed, and its must-reject list has been read as
what it is — the M2 conformance suite, stated in advance.
