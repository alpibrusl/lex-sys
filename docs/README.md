# Design documents

Design work lands here before any compiler code exists.

| Doc | Purpose | Status |
|---|---|---|
| `linearity-and-effects.md` | The core type-system rules: linear/affine ownership, capability-typed effects, how they unify, and the cases that **must** be rejected. Worked examples throughout. | not written |
| `canonical-ast.md` | AST shape, canonicalisation rules, per-unit identity (signature vs body hashing), and the determinism invariants. | not written |
| `memory-model.md` | Regions/arenas, what escapes, the escape hatches (refcount / generational refs) and their runtime cost. | not written |
| `defined-behaviour.md` | Every place C/Rust leave behaviour open, and what we define it to instead. Integer overflow, evaluation order, layout. | not written |

`linearity-and-effects.md` is the gating artifact: it is the decision set that
determines whether this is a three-month prototype or a three-year project, and
nothing in M2 should start before it is settled.
