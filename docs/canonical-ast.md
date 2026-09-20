# The canonical AST and per-unit identity

> **Status: proposed.** Written alongside the first implementation of hashing
> (M3, #1). The rules below are what `lex-sys-id` implements; the parts that
> are *not yet contracts* are called out in §8, and nothing here is frozen
> until that section is empty.

lex-sys claims a "canonical, content-addressable AST, designed in from day one"
(#1). M0 shaped the AST for it and M1 left that shape intact. This document is
the other half: what exactly gets hashed, what two programs must agree on for
their hashes to agree, and — the part that matters most — what a hash is
*allowed* to change with.

---

## 1. What a unit is

A **unit** is one top-level declaration: a function, a struct, or an enum. Each
has an identity of its own, independent of the file it sits in and of what sits
beside it.

That is the whole point of *per-unit* identity. Moving a function between files,
or reordering declarations, changes nothing about it. A unit is not a line
range.

---

## 2. Two identities, not one

Every function has **two** hashes, and the split is load-bearing:

| | covers | changes when |
|---|---|---|
| `SigId` | the signature: name, parameter types, return type | the contract a caller depends on changes |
| `BodyId` | the implementation | the code changes, however little |

A caller depends on a callee's `SigId` and on nothing else about it. So
rewriting a function's body — a faster algorithm, a clearer loop — leaves every
caller's identity untouched. Only a change a caller could *observe* propagates.

This is also what makes the graph acyclic. A body refers to its callees by
their `SigId`, and a `SigId` never depends on a body, so mutual recursion
cannot produce a cycle:

```
fn even(n: int) -> bool { if n == 0 { return true; } return odd(n - 1); }
fn odd(n: int) -> bool  { if n == 0 { return false; } return even(n - 1); }
```

`BodyId(even)` contains `SigId(odd)`, `BodyId(odd)` contains `SigId(even)`, and
neither `SigId` contains either body. A Merkle graph over cyclic source.

Struct and enum declarations have a single `TypeId`, since they have no body to
separate. A foreign declaration (`extern fn`) is a signature with no body, so
its `SigId` and `BodyId` are the same hash — there is nothing else a body hash
could be a hash of.

---

## 2a. The printer is the other direction

`lex-sys print <file>` renders a parsed unit back to text. It is the
AST→text half of the same pipeline: a store that addresses code by hash
needs a way to show a declaration it fetched, and that rendering has to be
canonical or the display would depend on who stored it.

Two contracts, both enforced by a test that walks every `.ls` file in the
repository:

* **Identity-preserving.** Parsing the output gives back the same `SigId`,
  `BodyId` and `TypeId` for every declaration.
* **Idempotent.** Printing the output again changes nothing.

The printer is deliberately **not** a formatter. §3 below says comments never
reach the AST, precisely so that formatting cannot change a hash — which
means anything built on the AST cannot put them back, and a `fmt` that
silently deleted every comment in a file would be a bad trade. Rendering a
stored declaration, where there were no comments to lose, is the job this
does.

Writing it found two places where the canonical form is decided by the
*grammar* rather than by the tree, both of which the round-trip test caught
rather than review:

* a struct literal in an `if`, `while` or `match` head has to keep its
  parentheses, because its braces would otherwise be taken for the block —
  and parentheses leave no node behind to remember that;
* `-` immediately before an integer token is one literal rather than a
  negation of one (which is how `-9223372036854775808` is writable at all),
  so a negation *of* a literal has to stay visibly apart from one.

---

## 3. What must not change a hash

These are the properties the AST shape was built for, and each has a test:

- **Formatting.** Whitespace, line breaks and indentation never reach the AST.
- **Comments.** Discarded in the lexer.
- **Redundant parentheses.** `(a + b) * c` and `(((a + b)) * c)` are one tree;
  grouping leaves no node behind.
- **Literal spelling.** `007`, `7` and `1_000` are values, not text. `0x7` will
  be too, when it exists.
- **Position in the file.** Spans live in side tables, never inside a node.
- **Neighbours.** A unit hashes alone. Adding, removing or reordering other
  declarations changes nothing.
- **Local names.** See §5 — this one is a decision rather than a consequence.
- **A redundant `val`.** A declaration's mode is `val` unless it says
  otherwise, so writing the word on a type whose members are all `val` asserts
  exactly what absence already checks — and a `val` that was *not* true is
  refused before anything hashes it. The two declarations are one type.

## 3.1 What must change a hash

- Any operator, literal value, or control-flow structure.
- Any type in a signature, and the *order* of parameters.
- A callee's signature, transitively.
- Field and variant order in a declaration, because they are positional to the
  backend and observable through construction.
- A declaration's `res` (`docs/linearity-and-effects.md` §3). A linear type is
  not the same type as a copyable one, and every caller can tell.

---

## 4. The encoding

A hash is taken over a byte string, and the byte string is what has to be
canonical. The rules:

1. **Every node starts with a one-byte tag.** Tags are assigned explicitly and
   never reused; §8 explains why they are not yet frozen.
2. **Integers are fixed-width little-endian.** `i64` as eight bytes, counts as
   four. No variable-length encoding, because two encoders must not be able to
   disagree about the short form.
3. **Strings are length-prefixed UTF-8** — four bytes of length, then the
   bytes. Never null-terminated, so a name cannot smuggle a delimiter.
4. **Sequences are length-prefixed**, then their elements in order.
5. **Nested hashes go in as their 32 raw bytes.**

### 4.1 Names are text, never interner indices

The AST interns names to dense indices, and those indices are assigned in order
of first appearance *across the whole file*. They are deterministic for one
file and meaningless between two. Encoding an index would make a unit's hash
depend on what else the file happened to mention first — exactly the neighbour
dependence §1 exists to remove.

So the encoding carries the name's **bytes**. This is the one place where the
convenient in-memory representation is the wrong thing to hash, and writing
this document is how it was noticed.

---

## 5. Local names do not matter; the decision and the cost

**Decision: bodies are hashed up to alpha-equivalence.** Renaming a local
binding does not change `BodyId`.

```
fn f(n: int) -> int { let doubled = n * 2; return doubled; }
fn f(n: int) -> int { let d = n * 2; return d; }
```

Both have the same `BodyId`. A reference to a bound name encodes the *binder's
position* — how many binders back it was introduced — rather than its text. A
reference to something not bound locally, such as a function, encodes its
identity instead.

The case for: a rename is a refactor that cannot change what a function
computes, and an attestation that says "this function does X" should survive
one. The case against: it costs a scope walk during hashing, and it means two
textually different files can produce byte-identical hashes, which surprises
anyone expecting a hash of the source.

The second is a real cost and is accepted deliberately: this is a hash of a
*program*, not of a file. Anyone wanting the latter should hash the file.

Parameter names are excluded from `SigId` for the same reason — lex-sys has no
named arguments, so a caller cannot observe them. Generic parameter names are
excluded and their *count* included, since `fn f[T](x: T)` and `fn f[U](x: U)`
differ in no way a caller can see.

---

## 6. Domain separation

Every hash begins with a domain tag, so a signature's bytes can never be
mistaken for a body's even if they coincide:

```
lex-sys.sig.v1      a function signature
lex-sys.body.v1     a function body
lex-sys.type.v1     a struct or enum declaration
```

The `v1` is not decoration. When a rule in this document changes, the version
changes with it, and old hashes stay readable as what they were rather than
becoming silently wrong.

The hash is **BLAKE3**, 32 bytes, rendered as lowercase hex and abbreviated to
16 characters for display only. Never for comparison.

---

## 7. What this buys

Nothing in M3 consumes these hashes yet, which is worth saying plainly. They
are built now because:

- **The AST shape can be checked.** M0 claimed the AST was built for
  canonicalisation. Until something hashed it, that was an assertion. It is now
  a test suite.
- **Attestation needs them** (#1), and so does any incremental build, cache or
  registry. Each of those is easier to add to a compiler that already has
  stable identities than to retrofit.
- **The self-hosting spike depends on exactly this.** #1 proposes porting
  `lex-ast`/`lex-vcs` canonical forms and checking byte-identical
  `OpId`/`SigId`/`StageId` against a 136k-op corpus. A lex-sys that cannot hash
  its own AST cannot run that experiment.

---

## 8. Not yet contracts

Per `docs/INVARIANTS.md`'s convention in lex-lang: these are implemented but
**not** promised, and may change without a version bump until this section is
empty.

- **Tag values.** The byte assigned to each node kind is stable within a build
  and not yet frozen across releases. Freezing them means writing them down
  here as a table, which is worth doing once the AST stops growing — every
  milestone from M2 on adds nodes.
- **`BodyId` across a type-checker change.** Bodies hash from the AST, not the
  typed IR, so inference changes do not move them today. Whether that survives
  M2 — where a body's meaning depends on effects and linearity that are not
  syntactically present — is open.
- **Cross-version stability.** No claim is made that a hash from this build
  matches one from any other build. The domain tags exist so that claim can be
  made later.
- **Field order where the declaration decides it.** A struct literal lists its
  fields in whatever order it likes and the checker reorders them, so
  `P { x: 1, y: 2 }` and `P { y: 2, x: 1 }` are the same value — and they hash
  differently. A destructuring pattern has the same gap for the same reason:
  `let P { x, y } = p` and `let P { y, x } = p` bind the same values. Both
  could be canonicalised by sorting into declaration order, which is not known
  where the encoder runs; neither is, and the tests say so rather than
  asserting the property does not exist.
