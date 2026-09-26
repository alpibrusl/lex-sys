# Self-hosting: the spike, and the decision it was for

`bootstrap.md` named the reason the v0 compiler is written in Rust
(*"Cranelift is a Rust library"*) and, in the same breath, named the
cheap experiment that would tell this project whether that reason is
load-bearing forever: *"port the `lex-ast`/`lex-vcs` canonical forms and
check byte-identical `OpId`/`SigId`/`StageId` over the existing op-log
corpus."* `ROADMAP.md`'s "Beyond" section repeats the same line. This
document runs that experiment, on the corpus this sandbox actually has
rather than the one the original note assumed, and answers the
question `bootstrap.md` deferred: is self-hosting worth planning next?

**No — not yet, and not because anything found here is a hard
blocker.** Every concrete thing checked came back feasible. What is
missing is an asker (`AGENTS.md` §7): no lex-sys program needs to
compute a lex-lang `SigId`, and no consumer is waiting on a
lex-sys-language compiler. This is the same discipline `docs/vcs.md`
§8 already applied to whole-function merge and typed issues —
*"has no asker in this repository yet... not started"* — applied here
to a much larger piece of unrequested work.

---

## 1. Two different questions this note does not conflate

*"Self-host the toolchain"* and *"make `lex-sys-vcs`'s `OpId` match
`lex-vcs`'s"* sound like the same idea and are not.

`docs/vcs.md` §5 already answered the second one, on contact, while
building `lex-sys-vcs`: **`OpId` is BLAKE3, not SHA-256**, deliberately
— *"matching `lex-vcs`'s algorithm would have meant a second hash
dependency for no reason but appearance."* `lex-sys-vcs` hashes
lex-sys programs; a lex-sys program and a Lex program are never the
same bytes, so there was never a reason for their content hashes to
agree, and none was sought.

What `bootstrap.md` actually asks is the first, larger question: could
*lex-sys itself* — the language — eventually be the implementation
language of its own compiler (today's `lex-sys-syntax`/`lex-sys-ir`/
`lex-sys-codegen*`, ~29,400 lines of Rust, counted below), the way a
self-hosted compiler is normally understood. That is what the rest of
this document measures.

---

## 2. The corpus this sandbox actually has

`ROADMAP.md`'s line says *"the existing ~136k-op corpus."* No such
corpus is checked into any of the four repositories this session can
reach — `lex-lang`, `lex-sys`, `lex-os`, `lex-gpu` — confirmed by
searching each for an op-log store, not assumed from the absence of a
memory of one. Whatever corpus that line originally meant lives
outside this sandbox (a production op-log from real use, most likely),
and this spike does not have access to it.

What does exist, real and checked into `lex-lang` itself: 31 real
`.lex` files (`examples/`, plus the fuzz seed corpus under
`fuzz/corpus/`), two of which are deliberately malformed fuzz seeds
that do not parse (`expected type expression, got Some(Fn)` — a fuzzer
seed testing the parser's own error path, not a program). The other 29
parse into 168 real `Stage`s (`fn`/`type` declarations) through the
real, unmodified `lex-syntax`/`lex-ast` pipeline.

This changes what is actually testable. Diffing a lex-sys hash against
a lex-lang hash for the *same* corpus was never going to say anything
— different languages, different ASTs, no reason to expect agreement.
What is testable, and what the "byte-identical" phrasing in
`bootstrap.md` is actually insurance against, is narrower and more
useful: **is the canonicalization *algorithm* — RFC-8785-flavored JSON,
SHA-256, the specific `SigId`/`StageId` composition — specified clearly
enough, in `lex-ast`'s own source and doc comments, that an independent
implementation reproduces it exactly?** A self-hosted port would be
exactly such an independent implementation, in a different language;
if the algorithm cannot be reproduced byte-for-byte from its own
documented spec in the *same* language, porting it to a different one
has no chance.

---

## 3. The experiment

A from-scratch Rust reimplementation of `canon_json`'s writer (UTF-8,
no whitespace, object keys sorted byte-wise, `serde_json`'s default
number rendering, JSON string escaping including `\u00XX` for control
bytes) and of `sig_id`/`stage_id`'s composition (read from
`lex-ast/src/lib.rs`'s own doc comments: *"SigId: SHA-256 over
canonical_json({name, input_types, output_type, effects})"*,
*"StageId = SHA-256(structural_sig_hash || implementation_hash)"*) —
written without calling `lex_ast::canon_json` or `lex_ast::sig_id`
itself, only depending on the real `lex_ast::Stage` type so both sides
walk the identical parsed data. Run over every real `Stage` the 29
parseable `.lex` files produce, diffed against the real crate's own
`sig_id(stage)` / `stage_id(stage)`.

```
corpus: 31 real .lex files
parse failure: fuzz/corpus/parser/seed_02.lex (deliberately malformed)
parse failure: fuzz/corpus/type_checker/seed_02.lex (deliberately malformed)
parse failures (excluded): 2
stages examined: 168
SigId byte-identical: 168/168
StageId byte-identical: 168/168
RESULT: every SigId and StageId reproduced byte-for-byte.
```

**168 for 168.** The canonicalization algorithm is specified precisely
enough, in the source it already has, to reproduce without the
original code — the property a port needs and the property
`bootstrap.md`'s "byte-identical" phrasing was actually checking for.
This is the one part of the spike with a clean, positive, falsifiable
result.

---

## 4. What else a self-hosted toolchain would need, checked rather than assumed

Three more questions, each answered against this repository's own real
code rather than guessed:

**Does lex-sys support what a hand-written compiler needs structurally?**
Recursion — `fn fib(n: int) -> [] int { if n < 2 { return n; } return
fib(n - 1) + fib(n - 2); }`, built and run in this sandbox, returns 55
for `fib(10)`. Heap-allocated recursive data (an AST node needs a
`Box`ed child) — already proven, not hypothetical: `examples/tree.ls`
is a binary search tree, exactly this shape, already in the corpus
`docs/README.md` calls *"why a language needs a heap at all."* Neither
is a new risk.

**What is the actual size of what would move?** The Rust crates that
are the compiler today, real line counts:

| Crate | Lines |
|---|---|
| `lex-sys-syntax` | 5,318 |
| `lex-sys-ir` | 12,282 |
| `lex-sys-codegen` | 3,397 |
| `lex-sys-codegen-llvm` | 4,615 |
| `lex-sys-types` | 704 |
| `lex-sys-id` | 1,909 |
| `lex-sys` (CLI) | 1,143 |
| **Total** | **29,368** |

That is the scale of a full port, not a slice — an order of magnitude
past anything this repository has ported in one piece before (§2 of
`docs/vcs.md` measured `lex-vcs`'s *diff*-facing code at 1,477 lines as
the largest prior comparison point).

**Does `bootstrap.md`'s own Cranelift blocker still hold?** Yes, and it
is not a soft one: Cranelift and LLVM are libraries with Rust (and C++)
APIs — builder patterns, trait objects, complex owned types crossing
by value — none of which a foreign call can cross today
(`docs/reach.md` §3: *"a foreign result is `int`, `bool` or `()`"*).
Reaching either from `.ls` directly is not a missing convenience, it is
outside what `extern fn` can express at all. But `docs/reach.md` §3.4's
own note that `fork` is already reachable through `Ffi("libc")` points
at the actual escape hatch a self-hosted backend would take: emit
textual assembly (a `[byte]` slice, no different from any other output
this language already writes) and shell out to the system `as`/`ld`
the way an early self-hosting C compiler did, rather than reimplement
a code generator. Untried here — a real slice of work in its own
right — but not blocked by anything this language currently lacks.

---

## 5. The decision

Nothing this spike checked is a wall. The hashing algorithm ports
byte-for-byte from its own spec (§3). Recursion and heap-allocated
trees, the structural minimum, already work (§4). The Cranelift/LLVM
dependency that keeps v0 in Rust has a named way around it — shelling
out to `as`/`ld` — that does not require reimplementing a code
generator (§4). What is missing is scale (29,368 real lines, §4) and,
more to the point, an asker: nothing in this repository today needs a
lex-sys compiler written in lex-sys, and `AGENTS.md` §7's rule against
building what nothing asks for applies at this size exactly as it does
at `docs/vcs.md`'s smaller ones.

**Decision: stay on Rust for the compiler. Revisit this document,
rather than re-running the spike, the day a concrete asker exists** —
most plausibly `lex-os`'s own port maturing to the point where running
the *lex-sys compiler itself* inside a sealed box (rather than just
lex-sys *programs*) becomes something that box's own trust model
needs. Nothing in this document's findings would need to change before
that day; only the "no asker yet" line would.
