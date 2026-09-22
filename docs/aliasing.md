# Does `&!` mean unique? Measured, and the answer is no — twice over

> **Status: a documented no, and the README asked a question its own
> table had already answered.**
>
> `ROADMAP.md`'s first *what is next* row, `README.md`'s performance
> section and `gpu.md` §4 all carry the same open question: `&!` does
> not mean what Rust's `&mut` means, `both(s, s)` compiles, and whether
> to change that is "a real question with a real cost". The stated cost
> was *"it refuses programs that compile today"*.
>
> **That is not the cost.** Across 82 programs, the rule refuses
> **one fixture**, and that fixture exists to document the behaviour
> being changed. The real cost is in two places neither row named:
>
> 1. **Provenance across a call is a borrow checker.** Two of the three
>    ways to alias a `&!` today close syntactically. The third does not,
>    and closing it means tracking where a returned reference came
>    from — the machinery `README.md`'s design commitments rule out by
>    name, two sections above the question.
> 2. **The backend cannot hold the answer.** Cranelift 0.121.2 has no
>    `noalias`. Its whole aliasing vocabulary is three fixed WebAssembly
>    regions, and `AbiParam` carries no attribute for it. A proved fact
>    would have nowhere to go.
>
> So the performance half of the question is unreachable at *both* ends,
> independently, and the correctness half is a research budget rather
> than a pass. §6 is what would change that.

---

## 1. What `&!` means today

`slicing.md` §4 already said it, in the opposite direction from the
README:

> Two copies of one `&!r` are two copies of one **pointer**, so writes
> through them alias correctly rather than racing to be last writer.

`&!` is a lock on the **binding** for a block. `borrow mut x as &!r in
{ … }` freezes `x` — nothing may read it, move it, assign to it or
borrow it again while the block runs, and `linear.rs` enforces all four
(`tests/reject/two_unique_borrows.ls` and four siblings). That is a real
guarantee and it is the one the language delivers.

What it is **not** is a promise about *references*. A `&!r T` value is
`val`: copyable, storable, returnable. Once one exists, the lock on the
binding says nothing about how many copies of the pointer are in flight.

---

## 2. The corpus, counted

A temporary probe in `crates/lex-sys-ir/src/lib.rs` — at the call-argument
loop, and at the lowering of a name to `Expr::Load` — reported every
reference-typed argument's root place and every read of a `&!` binding,
under an environment variable. The corpus is every program the repository
builds: 11 single-file examples, 8 example directories, 53 `tests/accept/`
fixtures and 10 benchmark programs, **82 programs**, each compiled with
`--std`. Counts below are distinct source positions, with the standard
library's own sites counted once rather than once per program.

| | corpus | of which `std` |
|---|---:|---:|
| call sites passing two or more references | 121 | 15 |
| …passing two or more **unique** references | 34 | 0 |
| …where two reference arguments are the same place | **1** | 0 |
| …where that pair involves a `&!` | **0** | 0 |
| places a `&!` binding is read | 1,943 | 154 |
| places a `&!` is copied into a new binding | **2** | 0 |
| functions returning a `&!` in a parameter's region | **1** | 1 |

The counts are from **before** §3's four fixtures were added. Those
fixtures alias on purpose, so they would each add a row to the table;
leaving them out is what makes it a measurement of the corpus rather
than of this document.

The single aliasing call site is `examples/sort/sort.ls:148`:

```
bytes.compare(text[a_at..a_at + a_len], text[b_at..b_at + b_len])
```

Two overlapping-by-construction views of one buffer, both **shared**.
Rust accepts it, C accepts it, and no uniqueness rule would refuse it.
So: in every line of lex-sys this repository has, **nothing aliases a
unique reference except the fixtures written to prove it can**.

That is the number the ROADMAP row wanted, and it says the rule is cheap.
§3 is why cheap is not the same as available.

---

## 3. Three routes, and they are not the same difficulty

Each is a `tests/accept/` fixture, accepted today, printing `2` — the
second write, read back through the first reference.

### Route 1 — the same place twice

```
both(s, s)      // tests/accept/aliasing_same_place_twice.ls
```

Closed by a syntactic rule: no two reference arguments of one call may
share a root place, where the root is found by walking through field
projections, indexes and subslices to the binding underneath. Cost, by
§2: **zero call sites** in the corpus. It is the version everyone
reaches for first, and it is worth exactly as much as §3.2 leaves it.

### Route 2 — a copy into another binding

```
let t = s;      // tests/accept/aliasing_through_a_copy.ls
both(s, t)
```

Two roots, one object. Route 1's rule does not see it, and neither does
any rule that reads one expression at a time.

Closing it means a `&!` reference stops being `val`: copying one becomes
a **move**, and every ordinary use becomes an implicit *reborrow* — the
reference handed to a call is a fresh borrow that ends when the call
does, which is what Rust does silently at every `&mut` argument.

The reborrow is not optional. Without it the corpus loses **1,943**
reads, 154 of them inside the standard library; `examples/tour.ls` alone
reads a `&!` binding in 372 distinct places. With it, the cost of the
move rule is the **2** places that copy a `&!` into a new binding, both
of them in `tests/accept/unique_borrow.ls`, whose comment says what it
is doing:

> `&!r` is `val`, so it copies — and the copies are copies of one
> *pointer*, so writes through them alias rather than racing to be the
> last writer.

A slice makes the same copy without a binding:
`alloc_slice[r](4, s)` is four aliases of one object
(`tests/accept/aliasing_in_a_slice_of_references.ls`). A **struct**
cannot: a type declaration takes no region parameters, so a field can
only name a region that needs none — `&static`, read-only data that
aliases no arena. That asymmetry is the reason route 2 is bounded at all.

### Route 3 — laundered through a return

```
fn head[&a](s: &!a [int]) -> [] &!a [int] { return s[0..1]; }
both(head(s), s)          // tests/accept/aliasing_through_a_return.ls
```

This one has no syntactic fix. The caller sees two expressions of the
same type and no reason to think they touch the same bytes, because
nothing in `head`'s signature says the result borrows the argument.

**Regions do not say it.** `-> [] &!a [int]` says the result lives in
the same *arena* as the parameter — and so does every other reference
into that arena, including ones with nothing to do with `s`. A rule that
treated same-region as same-object would refuse two unrelated slices of
one arena, which is most of what arenas are for. Region identity is not
provenance.

Knowing that `head`'s result borrows `head`'s argument, and carrying
that through the call, *is* the fact Rust's lifetimes encode and its
borrow checker propagates.

---

## 4. What each closure would cost

### 4.1 Route 1 alone: free, and worth nothing

Zero programs refused, and `&!` still does not mean unique, because
routes 2 and 3 each reach the same aliasing in one extra line. A rule
that refuses the obvious spelling of a thing it permits is worse than
no rule: it tells a reader — and `AGENTS.md` tells an agent — that
aliasing unique references is refused, which would then be false.

### 4.2 Routes 1 and 2: cheap, and still not unique

Two fixture sites, plus implicit reborrow, which is real compiler work:
1,943 read sites have to become reborrows that end at the right point,
and "the right point" is a scope question the checker currently never
asks about references. Route 3 survives, so `&!` still does not mean
unique and no `noalias` follows.

### 4.3 Route 3: the non-goal

Two ways to close it:

* **Ban the shape.** Refuse any function returning a `&!` in a
  parameter's region. The corpus has exactly one such function —
  `std.buffer.room`, added because `fs_read` writes into a buffer the
  caller must hand it as room before it knows the length
  (`porting.md` §9) — and exactly one call site, `examples/sort`. So the
  ban costs the standard library a function that a port needed and
  nothing replaces, in order to rule out a pattern that the same port
  proved is the honest way to write it.
* **Track provenance.** The signature says which argument the result
  borrows from, and the checker propagates it. That is lifetimes, and
  propagating them is a borrow checker.

`README.md`'s own design commitments, two sections above the open
question:

| Area | Commitment | Why |
|---|---|---|
| Memory | Linear/affine types + regions/arenas — **not** an NLL borrow checker | Local, cheap, total to check |

and its non-goals: *"No trait-system maximalism, no GATs, no
specialisation, **no borrow checker**."*

The question was answered before it was asked. What the measurement adds
is *which* fact is the expensive one, so that the answer is a reason
rather than a preference.

---

## 5. The other end: the backend cannot take the answer

Suppose the checker proved it anyway. The fact has one consumer — the
code generator — and lex-sys generates through Cranelift 0.121.2, which
does not have the concept.

* `ir::AbiParam` is `{ value_type, purpose, extension }`. `purpose` is
  `Normal | StructArgument | StructReturn | VMContext`. There is no
  aliasing attribute on a parameter.
* The only aliasing vocabulary in the IR is `MemFlags`'
  `AliasRegion::{Heap, Table, Vmctx}` — three fixed disjoint regions for
  WebAssembly's memory model, not a per-pointer `restrict`.
* `noalias` does not appear anywhere in `cranelift-codegen`'s `ir`.

So the performance claim the README retracted cannot be recovered by
fixing the checker alone. It needs the checker **and** a backend that
has the attribute — LLVM, which the same table already names for
release builds and `gpu.md` §1 places outside the next slices.

Two independent blockers is the finding. Either one alone would make
this a "not yet"; both make it a different project.

---

## 6. What would change the answer

In the order that would make it worth reopening:

| | what it would take | what it would then buy |
|---|---|---|
| **An LLVM backend** | The thing `gpu.md` concluded is the only way to GPU speed, and the same conclusion arrives here from a different direction | A consumer for the fact. Until it exists, proving uniqueness is provably worthless to the optimiser |
| **Parallel lanes** | Threads, or the GPU of `gpu.md` §4 | The *correctness* argument, which today is nil: on one thread, two aliasing writes are defined and ordered. Two lanes writing through aliasing references is the race a checker should catch, and that is when this stops being about speed |
| **Provenance in signatures** | Lifetimes, in some form the non-goals can live with | Route 3, and with it the word "unique" |

The first two are conditions, not work items. If both arrive, the third
is a milestone with a design document of its own, and this one is its
§1.

Until then `&!` means what `slicing.md` §4 said it means — a lock on a
binding — and the four fixtures in §3 keep that honest: each is accepted
today, so the day one of them is refused, it is a red build and a
deliberate decision rather than a drift.

---

## 7. What this does not say

* **Not that aliasing is safe.** It is *defined* — two writes through
  one pointer, in program order, last one wins — which is what
  `defined-behaviour.md` promises and all it promises. A program that
  aliases by accident is still wrong; nothing here catches it.
* **Not that the rule is hard.** Routes 1 and 2 are a few hundred lines.
  The measurement says they are cheap **and** insufficient, which is a
  worse combination than expensive, because it is the one that invites
  shipping half.
* **Not that Rust is ahead on the axis the README claimed.** It is, and
  `README.md` says so. This document is about what it would take to
  catch up, not about whether the gap is real.
