# Content-addressed VCS: what lex-sys would need, and what it would not

> **Status: §7's plateau question is answered, and the gate, op log,
> attestation and signing are built.** `crates/lex-sys-vcs` exists: the
> `Operation` vocabulary (§4's scoped-down `AddFunction`/
> `RemoveFunction`/`ModifyBody`, no `budget_cost`), the edition tag
> (§6), canonical BLAKE3 identity (checked against real `lex-sys-id`
> hashes, not invented strings), a `gate.rs` that type-checks a
> candidate program with no code changed at the boundary from `lex-vcs`
> (§3's own claim, now checkable), a loose-file op log, and a
> hash-chained attestation log sealed with Ed25519 (`ed25519-dalek`,
> the same crate `lex-os-audit` uses — **not** `std.ed25519`,
> `docs/ed25519.md`'s own module for a different consumer). What
> remains from §3's "largely unmodified" list — merge, merge sessions,
> issues — is not built yet; §8 names what is next.
>
> `ROADMAP.md`'s own "lex-vcs" row measured a plateau in the effect
> vocabulary and the builtin surface (`hash-stability.md`) and ended on
> an open question: *"whether 27 commits is enough of a plateau... is
> the open question this row now asks, not whether one has happened at
> all."* §7 below answers it. Everything before that is the design the
> answer unblocked.

---

## 1. What this is answering

`README.md`'s design commitments table lists the AST as "canonical,
content-addressable, stable per-unit identity — designed in, never
retrofitted," and `lex-sys ids` prints exactly that per declaration.
What lex-sys does not have is anywhere to *put* those identities: a
program here is still a set of files under ordinary git, diffed and
merged line by line, the one part of `lexlang.org/manifesto`'s own
argument (§IV, *"discard line-based version control... content-
addressed AST nodes, stable identities across reformats"*) this
repository has stated the identity half of and never finished the
other half of.

`agent-errors.md` §2 found the concrete cost of that gap already:
lex-lang's `rule_tag`/`suggested_transform` pair is the structured
repair hint an agent loop reads instead of re-deriving a fix from
prose, and *"the `rule_tag` half transfers whole, and `suggested_
transform` does not — its own guidelines page shows it beside
`rule_tag` in the checker's JSON, and in the source it lives in
`lex-store`/`lex-vcs`/`lex-lsp` as an attestation against an op id,
which is machinery lex-sys deliberately does not have."* A store is
not a nice-to-have layered on top of the checker; it is what the
other half of that repair loop is made of.

`lex-lang`'s `crates/lex-vcs` already built this once. The question
this document answers is not *whether* to have an op-log VCS — that
argument is `agent-errors.md` §2's, made before this document existed
— it is **how much of `lex-vcs` lex-sys can reuse, and what has to be
native**, so that building it is a scoped slice rather than a second
multi-crate project.

---

## 2. What `lex-vcs` already is, read from the source rather than the pitch

`ROADMAP.md`'s #55 row estimated **81% language-agnostic** by reading
`crates/lex-vcs/src/`. Reading the same source for this document finds
the same number a different way — by naming exactly which files earn
it and which do not, rather than a percentage of the whole crate.

**Generic already, keyed on `String`/`BTreeSet<String>`, and said to
be on purpose:**

| File | What it is | Why it is already generic |
|---|---|---|
| `operation.rs` (1046 ll) | `Operation`, `OperationKind`, `OpId`, `SigId`, `StageId`, `EffectSet`, `BlobId`, `ModuleRef` | Every one of these types is a type alias to `String` or `BTreeSet<String>` — read straight from the source: *"we keep it as `String` here so this crate has no dependency on `lex-store`'s internals"* (`SigId`), *"kept as a string so this crate doesn't pull in `lex-syntax`'s parser"* (`ModuleRef`). The decoupling is stated, not incidental |
| `apply.rs` + `gate.rs` | Applies an `Operation` to a store state; wraps that with a type-check pass | `gate.rs`'s own header: the gate runs `lex_types::check_program` against the *candidate* program and rejects the op if it does not typecheck. Nothing about the gate's own logic is Lex-specific — it calls out to *a* checker and reads back *a* verdict |
| `attestation.rs`, `signing.rs`, `merge.rs`, `merge_session.rs`, `issue.rs`, `predicate.rs`, `op_log.rs`, `history_index.rs`, `migrate.rs` | Attestation log, Ed25519 signing (`Keypair`/`verify_message`), whole-function merge, multi-file merge sessions, typed issues, predicate branches, the op log itself, and format migration | All key on `OpId`/`SigId`/`StageId`/`String`, none import `lex_ast`. `lex_vcs::signing` is, incidentally, exactly the Ed25519 verification code `docs/sha512.md` §5 named as unstarted for lex-sys's own runtime — a second, independent reason the two initiatives will eventually meet |

**Not generic, and named exactly because they are the exception:**

| File | Lines | What it walks |
|---|---|---|
| `compute_diff.rs` | 449 | `lex_ast::{CExpr, FnDecl, TypeDecl, Effect, ...}` — structural diff between two function bodies, node by node |
| `diff_to_ops.rs` | 617 | The same `CExpr`-shaped diff, turned into `OperationKind` values |
| `body_merge.rs` | 411 | Three-way structural merge of two `CExpr`s against a common base |

`449 + 617 + 411 = 1477` — `ROADMAP.md`'s *"only ~1,500 lines"* was not
a rounded guess, it is these three files exactly, and nothing else in
the crate names `lex_ast` at all.

---

## 3. What ports as-is, unmodified, and why that is more than it sounds

`Operation`/`OperationKind`/`OpId`/`SigId`/`StageId`/`EffectSet` need
no lex-sys-specific type at all: `lex-sys ids` already produces the
`String` content hashes `SigId`/`StageId` want, and an
`EffectSet` is already how `lex-sys authority` reports a row —
`docs/authority.md`'s own JSON output is a sorted list of effect-label
strings. A `AddFunction { sig_id, stage_id, effects, budget_cost,
in_file }` op is legible to lex-sys with no field renamed:

- `sig_id`/`stage_id`: `lex-sys-id`'s own hashes, unchanged.
- `effects`: the row `lex-sys authority`/`check --output json` already
  emits as strings.
- `in_file`: `many-files.md`'s own model — identity by content rather
  than location — is exactly what this field records for lex-lang's
  multi-module packages, and lex-sys already has multi-file programs
  with the same property.
- `budget_cost` (and `ModifyBody`'s `from_budget`/`to_budget`): the one
  field with **no** lex-sys equivalent, because `budget.md` settled
  `[budget]` as a language feature with a documented **no** — *"none
  of those is a property of a program's text."* lex-sys-sourced ops
  simply never populate it, and the field's own `Option` +
  `skip_serializing_if` discipline (already built so a pre-`#247` op
  keeps its `OpId` unchanged) is exactly the mechanism that makes
  "some producers never fill this in" cost nothing — it was built for
  a different reason and reads as though it were built for this one.

The apply→gate pipeline (`apply.rs`, `gate.rs`) ports as an *idea*
with no code changed even at the boundary: `gate.rs` asks one question
of the *candidate* program — does it typecheck — and lex-sys already
answers that question, at the same total, per-declaration granularity
`agent-errors.md` §1 measured. Swapping `lex_types::check_program` for
whatever calls into `lex-sys-ir`'s own checker is the entire seam.

Attestation, signing, merge sessions, issues, predicates, the op log
and history index: no seam at all, because none of them read an AST —
they read `OpId`s, `SigId`s, and the `String`s a `SigId` and an
`EffectSet` already are.

---

## 4. What has to be native, and what changes about it here

`compute_diff`, `diff_to_ops` and `body_merge` cannot be reused; they
have to be re-*written*, against `lex-sys-ir`'s own `Expr`/`FnDecl`
rather than `lex_ast::CExpr`. That is real, new work at roughly the
scale the existing files are — not a wrapper, not a trait
parameterization, because the node shapes genuinely differ (no
closures, no pattern-match arms shaped like Lex's, `region`/`Box`
where Lex has none of either) — but it is *bounded* work with a
precedent to check every case against: `fold.rs`'s own exhaustive,
no-wildcard walks over every `Expr` variant (built for static
reachability, `crypto.md` §6, and reused unmodified as the discipline
for this) are the shape `diff_expr`'s own match should take, so a
future `Expr` variant that goes unhandled is a build failure here the
same way it already is in `lex-sys-codegen-llvm`'s `body/expr.rs`.

**One real difference from lex-lang, and it does not need a new
invariant — `body_merge.rs` already built the thing that resolves
it.** `ROADMAP.md`'s own #55 row raised the concern first: *"in a
linear language two edits that each typecheck can together move a
value twice, so it will conflict far more often here than it does
there."* Reading `body_merge.rs` finds the answer was already written
into the design before lex-sys asked the question: *"It makes no type
judgement — that's the caller's job (the store type-checks the merged
body through the same gate every other write goes through); a body
that merges structurally but not by type is still a conflict."* A
merge that is structurally clean but consumes a linear value twice is
exactly the case §3's gate already exists to catch — nothing about
`gate.rs`'s design needs to change for lex-sys, because it was never
"trust the merge," it was "verify the candidate" from the start.

What *does* change is the **rate**, and that is worth measuring rather
than assuming, the same way `hash-stability.md` measured the plateau
rather than assuming one had happened. In a non-linear language, two
edits to disjoint subtrees that each typecheck are very likely to also
typecheck *together* — there is no shared resource for them to race
over. In lex-sys, "disjoint subtrees, each typechecks alone" is a
strictly weaker guarantee, because a linear value can be consumed once
in each of two structurally-disjoint edits and the merge will look
clean node-by-node right up until the gate runs. So: expect a body
merge here to report `Merged` and then fail the gate more often than
the same shape does in `lex-lang`, and expect the failure to land at
the merge site as `GateError::TypeError` rather than at a friendlier,
more specific "these two edits both move `x`" diagnostic — a real UX
gap against lex-lang's own experience, worth its own future slice
(teaching `body_merge`'s recursive case to recognize a linear binding
consumed on both sides *before* declaring the merge clean, rather than
after), and explicitly not attempted in this document.

`diff_to_ops`'s `OperationKind` vocabulary is narrower for lex-sys than
for lex-lang, in the same direction `budget_cost` already is: no
generics-with-trait-bounds to diff (lex-sys has `[T: val]`, not
lex-lang's trait system), no closures, no `comptime` macros to expand
before diffing (`README.md`'s own non-goal: *"no textual or proc
macros — macros break stable content-addressing"*, which if anything
makes lex-sys's diff *simpler* to get right than a macro-expanding
language's would be).

---

## 5. Where the code should live

Not a `Cargo.toml` dependency on `lex-lang`'s `lex-vcs` crate. The
`lex-os`/`lex-sys` relationship already set this precedent and said
why out loud: *"It shares the idea and no code: `lex-os` takes its
grant from `lex-lang` and checks `.lex`, and does not depend on this
repository at all."* The same reasoning applies here in the other
direction — a new `lex-sys-vcs` crate, built against this repository's
own types, sharing the *scheme* (canonical JSON, a content-addressed
`OpId`, `String`-keyed `SigId`/`StageId`/`EffectSet`) with no shared
code and no cross-repo `Cargo.toml` edge. **Built, and one detail
changed on contact**: `OpId` is BLAKE3, not `lex-vcs`'s SHA-256 —
`docs/canonical-ast.md` already chose BLAKE3 for `lex-sys-id`'s own
hashes and said why, and nothing in that reasoning is specific to an
AST node rather than an operation, so matching `lex-vcs`'s algorithm
would have meant a second hash dependency for no reason but
appearance. "Shares the idea and no code" turns out to mean sharing it
down to which hash function, not just which crate.

The honest tradeoff, stated rather than assumed away: two independently
maintained copies of `operation.rs`/`attestation.rs`/`signing.rs`/etc.
can drift, and a bug fixed in one will not automatically reach the
other. That is worth reconsidering — pulling the ~81% into a shared
`lex-vcs-core` crate both repositories depend on — only if the two
implementations turn out to need the same fix often enough that the
duplication is expensive to carry, not on the strength of "it would be
nice to share." Nothing in this repository's own history suggests that
yet: `lex-os` and `lex-sys` have shared **zero** lines of code since
that relationship was stated, and it has cost nothing measurable.

---

## 6. What edition-tagging buys, given what the plateau measurement found

`hash-stability.md` did not find one number, it found two that
disagree: the *encoder* has moved 20 times with the golden fixtures
observing none of it, while the *language* moved enough that 71% of
this repository's own historical `.ls` revisions no longer typecheck,
almost half of that from one label rename (`io` → `io_read`/
`io_write`). An op DAG keyed on content hashes that predate a label
rename does not become wrong when the rename lands — the hashes are
still correct hashes of what they were computed from — but a
consumer walking the DAG needs to know it is reading pre-rename
history, or a query like "every function whose row includes `io`"
silently misses everything written before the split.

`editions.md` already has the answer for the *checker* side of
exactly this (edition markers hold a vocabulary fixed per program), and
the op DAG should use the same marker rather than invent a second one:
every `Operation` this design writes should carry the edition its
`EffectSet` was computed under, from the day the DAG is born, not added
defensively later once a second edition already exists. This is cheap
now (one field, `Option`-skipped for the current edition the way
`intent_id` already is, so it costs nothing until there is a second
edition to distinguish) and expensive to retrofit (every historical
`OpId` would need to be reinterpreted rather than just read).

---

## 7. The plateau question, answered

`ROADMAP.md`'s own row asks whether 27 commits (`#93`–`#119`) touching
zero files that declare a builtin, an effect label, or a prelude type
— while an entire second backend was designed and built behind that
stillness — is *enough* of a plateau to stop waiting.

**Yes**, with one condition this document already built in: tag the
edition from the start (§6), so the thing a plateau is actually
insurance against — the vocabulary moving again later — costs a
reinterpretation rather than a rewrite if it happens. The alternative
to starting now is waiting for a second plateau to confirm the first
one generalizes, which is not a stronger falsification than the one
`hash-stability.md` already ran — it is the same measurement, taken
twice, on the belief that 27 is not enough of a sample size on its own.
That belief is not supportable from what actually moved in those 27
commits: an entire LLVM backend, `Net`'s outbound and inbound halves,
and `Ffi`/`extern fn`'s foreign-return-width fix — the kind of large,
invasive, cross-cutting change most likely to have *needed* a new
effect label or a new builtin if the vocabulary were still unsettled,
and none of it did. Waiting for a longer plateau to *feel* safer is
exactly the "assumed, not measured" failure mode this repository's own
documents keep catching in each other.

---

## 8. What is next

**Built**: the foundation (`Operation`/`OperationKind`/`OpId`/`SigId`/
`StageId`/`EffectSet`, canonical BLAKE3 identity, the edition tag from
§6, checked against real `lex-sys-id` output); the apply→gate pipeline
(`gate.rs`, checking §3's claim directly against real code — a
candidate program either typechecks or is refused with the same rule
tag `lex-sys check` would report, no `main`-shape check, since an
operation can be about a library declaration); a loose-file op log
(`op_log.rs`, one JSON file per `OpId`, idempotent, refusing a record
whose claimed identity disagrees with its own payload); and a
hash-chained attestation log (`attestation.rs`) that records the
gate's own verdict, sealable with Ed25519 via `ed25519-dalek` — the
same crate `lex-os-audit` uses, deliberately not `std.ed25519`
(`docs/ed25519.md`'s own module, built for a different consumer, §1
there). This is the meeting point §8's previous revision named: this
initiative and `docs/sha512.md` §5's deferred Ed25519 slice, closed by
the same crate on the Rust side and a purpose-built module on the `.ls`
side, each serving the consumer that actually needs it rather than one
serving both.

What is left from §3's "largely unmodified" list — whole-function
merge (`lex-vcs::merge`), multi-file merge sessions, typed issues,
predicate branches, the op log's own history index — has no asker in
this repository yet (`AGENTS.md` §7), and is not started. The
linearity-aware body merge named in §4 (recognizing a linear binding
consumed on both sides of a merge *before* declaring it clean, rather
than after) is the one item on that list with a design already written
down, should a consumer arrive before the others do.
