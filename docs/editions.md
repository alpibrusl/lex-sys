# Editions: what one can absorb, measured on this repository's past

> **Status: edition 2 is real.** *(Corrected: the marker shipped ahead
> of `Net`, not with it — #87, after the `parser.rs` split
> ([`CONTRIBUTING.md`](../CONTRIBUTING.md)) this paragraph originally
> said it needed.)* §7 turned out to need more than the marker: nothing
> in the compiler consumed `Ast::edition_of` outside the parser's own
> unit tests, so every place a written name resolves — a signature's
> parameters and return type, a `let` that destructures a struct, a
> struct literal, a variant constructor — needed the same "which
> declarations can this file see" question `resolve_type_at`'s `lookup`
> now answers, not only that one function
> ([`connect.md`](connect.md) §9). Edition 2 is `Net`'s first slice:
> the capability, its `net_out` label, and a `connect` that takes an
> address a caller already has. `Net`'s remaining two slices — name
> resolution, then inbound — are additions of the same shape and need
> nothing further here.
>
> The audit's L2 asked for **an edition marker, a label-alias path so a
> rename is a warning for one edition, and a vocabulary freeze once
> `Net` and `Exec` have added their labels.** The measurement below
> says the second part cannot work as asked. This language has no
> warnings, and more to the point **its rows are exact**. Reading an old
> file's `io` label as `io_read, io_write`, the two labels that replaced
> it, recovers **0 of the 45** old revisions that label split broke.
>
> What editions *can* do is narrower and still worth having. A change
> that **adds** something (a capability, a label, a builtin, a field on
> `Split`) can simply be absent from an older edition, which is exact
> and costs old files nothing. A change that **refines** something needs
> a migration tool. A change that **tightens** soundness needs neither,
> and should stay a refusal. `Net` is purely additive, so the 210 files
> that destructure `Split` need no edit at all when it lands.

---

## 1. What was asked

The audit, L2:

> **Language churn**: 71% of historical `.ls` revisions no longer check,
> and 42% of that comes from one label rename (`io` → `io_read`/`io_write`).
> Introduce a **language edition marker** (`edition 0.1;` per file or per
> program), and a **label-alias / deprecation path** so a rename is a
> warning for one edition instead of a break.

The freeze has to follow `Net` and `Exec`, since both add labels. And
[`net.md`](net.md) §4.1's decision adds a sixth field to `Split`, which
every `main` destructures completely:

```
error: `Split` has 5 fields, but this pattern names 4; destructuring takes the whole value apart
```

210 files in this repository have that pattern. Adding `net` to `Split`
with no edition breaks all of them. That makes this document the
prerequisite for `Net`, not an afterthought to it.

---

## 2. The past, replayed

`scripts/history.py` checks every distinct revision of every `.ls` file
under `std/` and `examples/` with today's compiler. A library file is
checked beside today's other library files and a `main` that does
nothing. An example in a directory is checked beside its siblings as
they were in the commit that introduced it.

| | |
|---|---:|
| distinct file revisions | **141** |
| today's compiler still reads | **83** (59%) |
| no longer checks | **58** (41%) |

Classified by the first refusal:

| Revisions | First refusal | What changed |
|---:|---|---|
| **45** | `effect-not-declared` | `io` split into `io_read` and `io_write` ([`standard-input.md`](standard-input.md) §2) |
| 8 | `expected [` | written before every signature declared a row |
| 3 | `arity-mismatch` | `putchar(c)` became `putchar(io, c)` |
| 2 | `linear-value-unconsumed` | linearity got stricter |

[`hash-stability.md`](hash-stability.md) §2 measured the same history
earlier and found 71% unreadable, over 117 revisions. It does not say
how a library file without a `main`, or one file of a multi-file
example, was checked, so the two figures cannot be reconciled exactly.
This harness gives each the program it belonged to. The class that
matters is the same in both: one label split is most of the unreadable
past.

Running this replay also found that `check --output json` answered
invalid JSON for every parse error, 20 of the 214 reject fixtures
([`agent-errors.md`](agent-errors.md) §5, fixed in #84).

---

## 3. An alias cannot absorb a split label

The audit's alias is the obvious mechanism: an edition-1 file's `io`
means `io_read, io_write`. `scripts/history.py --alias` applies exactly
that rewrite to every row of every broken revision and checks again.

| | |
|---|---:|
| broken revisions | 58 |
| recovered by reading `io` as both halves | **0** |
| now refused with `effect-declared-not-performed` | **41** |

The reason is one rule: **a row is exact**
([`linearity-and-effects.md`](linearity-and-effects.md) §7.3). A
function that only writes, and declared `[io]` when `io` meant both,
now declares `io_read`, which it never performs, and is refused. The
old `io` has no fixed translation. Its correct translation is whichever
half each body performs, and working that out is inference over the
body.

So an alias could only work in one of two ways, and each costs
something this language decided not to pay:

- **Relax exactness for old files.** A row in an old file would be an
  upper bound, while a row in a new file is exact. That gives one
  program two meanings of the same syntax, and the authority report
  would over-state what old code does.
- **Infer the halves.** A compiler that decides what `io` meant in each
  body is inferring rows, which §7.2 refuses across a boundary and this
  would do silently, on every build.

---

## 4. What a tool can absorb

The same inference is harmless when it runs **once, in a tool, and
writes its answer into the source**, where the checker then checks it
like any other row. `scripts/history.py --migrate` simulates such a tool.
It is driven by the checker's own refusals and applied only to the file
under test, and it knows two steps:

1. **Row repair:** add a label the body performs, drop one it does not,
   and drop the old `io` once either half is added.
2. **`Split` repair:** name the capabilities the pattern leaves out, and
   release each at once.

| | Revisions |
|---|---:|
| recovered by the two steps | **23** |
| the same row repair, needed in a *sibling* file the simulation does not edit | 7 |
| a builtin gained a capability parameter (`putchar(c)` → `putchar(io, c)`) | 11 |
| a linear value the old program never consumed | 17 |

So **30 of the 58** (52%) are mechanical: the 23 plus the 7 a real tool
would reach by editing every file. Peeling back the row changes exposed
a second layer. **15 revisions break only because `Split` grew**: it
gained `heap` in #22 and `args` in #24, and each time every `main` that
destructured it stopped checking. That is exactly what adding `net`
would do today.

The other two classes are not mechanical, and should not be:

- **A capability parameter** changes the signature of every function
  between `main` and the call. A tool could thread one through, but
  where authority enters a call graph is the programmer's decision.
  Having to write it was the point of the change.
- **Linearity getting stricter** means the old program leaked a value
  the language now refuses to leak. Migrating it automatically would
  mean choosing, for someone else, how to consume it.

---

## 5. Three kinds of change

| Kind | Example | What absorbs it |
|---|---|---|
| **Additive** | a field on `Split`, a new label, a new builtin | **the edition**: the new thing is absent in older editions. Exact, with no tool and no second meaning. Old files compile unchanged |
| **Refining** | `io` → `io_read`, `io_write`; a builtin gains a capability parameter | **a migration tool**, run once at an edition boundary. Old files are refused with the edition named and the tool as the fix |
| **Tightening** | a linearity rule that closes a leak | **nothing**. It stays a refusal, because the old program was wrong |

Additive changes are cheaper than §2 suggests, and they are also the
ones that have collided before. [`file-handles.md`](file-handles.md)
§4.2 recorded that three new prelude names cost 31 fixtures and one
`extern fn` in `examples/serve/`, because the new names were ones
programs were already using. Under an edition, the new names would not
exist in edition-1 files.

---

## 6. The decisions

### 6.1 The marker

```
edition 2;
```

- **Per file**, as the first item, before `module` if both are present,
  and at most once. Per file because `Split` is destructured in one
  file, and because a file is what one author writes at one time. A
  per-program edition would have to be agreed across files that are
  compiled together but written separately
  ([`many-files.md`](many-files.md)).
- **Absent means edition 1**, the language as it is today. No existing
  file changes, and the default can never move: a file with no marker
  means edition 1 forever.
- **An unknown edition is refused**, with a rule tag of its own. A
  compiler that guessed would be the silent downgrade the rest of this
  repository refuses.

### 6.2 What an edition may change

- **Only additions are held open across editions.** Every supported
  edition is today's language minus some additions, so the checker has
  one meaning of every construct. It asks which additions are visible
  in this file, and nothing else.
- **A refining change ends support for every earlier edition.** Files
  in those editions are refused with the edition named and
  `lex-sys migrate` as the fix. The supported window is therefore "since
  the last refining change", and it is visible in one table rather than
  implied by a pile of aliases.
- **A tightening is not an edition matter.** It lands as a refusal in
  every edition, as it does today.

### 6.3 Identity

A file's edition changes what its code means (edition 2's `split`
returns a different `Split`), so it belongs in the identity of what
that file declares. **Edition 1 contributes no bytes.** Every hash that
exists today stays exactly where it is, and moving a file to a later
edition is a deliberate, recorded step that moves its hashes once. That
is the plateau [`hash-stability.md`](hash-stability.md) §3 asked for:
declared, not waited for.

### 6.4 The freeze

Once `Net` and `Exec` have added their labels, the vocabulary of that
edition is **closed**. New labels, builtins and capabilities go into the
next edition, which costs old files nothing (§5). A refinement happens
only at an edition boundary, with a migration step to go with it.

### 6.5 The tool

`lex-sys migrate` is designed here and not built. §4's two steps are
its first two. It is worth building when the first refining change is
planned, and none is: `Net` is additive.

---

## 7. What `Net` needs from this

**Edition 2 is edition 1 plus `Net`**: the `net` field on `Split`, the
`net_in` and `net_out` labels, and the socket builtins. Built so far
(slice 1, [`connect.md`](connect.md) §9): the `net` field, `net_out`,
and `connect`. `net_in` and the rest of the socket builtins are slice
3. An edition-1 file's `split` returns today's five fields, and its
programs cannot reach `Net`, which they never could. The 210 files
that destructure `Split` stay as they are. A program that wants the
network writes `edition 2;` in the file whose `main` calls `split`.

`Split` is two declarations of one source name rather than a sixth
field appended to the existing one, and that is the part §6.2 did not
anticipate: `Split` is `res` by inference, so a sixth field nothing
edition-gated could unname would still have to be consumed by every
caller, which is exactly the break an addition must not cause. Name
resolution now asks two questions where it asked one — is this
declaration visible at all, and (new) is it the latest one this file's
edition can see — everywhere a written name resolves to a declaration,
not only in `resolve_type_at`.

A program can mix editions. Library files written in edition 1 are
used by an edition-2 `main` unchanged, since nothing in them mentions
`Net`. What an edition-1 file cannot do is name something only edition 2
has, and that is refused like any other unknown name.

---

## 8. What this does not do

- **The marker built nothing on its own.** *(Corrected: it shipped
  ahead of `Net`, not with it.)* `edition N;` parses and is stored per
  item alongside `docs/modules.md`'s module side table; an unknown
  edition is refused under its own tag (`unknown-edition`). That slice
  landed unobservable from any program a file could write, since
  nothing yet named anything edition 2 added. `Net`'s slice 1 is what
  makes edition 2 observable for the first time.
- **It does not migrate the past.** §2's 58 revisions stay unreadable,
  and nothing in this repository needs them.
- **It does not replace refusals with warnings.** A diagnostic here is
  still a refusal ([`AGENTS.md`](../AGENTS.md) §8). A later edition's
  convenience is never paid for with a softer earlier one.
- **It does not settle `Exec`.** `Exec` is expected to be additive in the
  same way as `Net`, but that is a prediction until its design exists.

---

## 9. The suite

| What | Pins |
|---|---|
| `scripts/history.py` | §2's table: 141 revisions, 58 unreadable, by first refusal |
| `scripts/history.py --alias` | §3: 0 of 58 recovered by reading `io` as both halves |
| `scripts/history.py --migrate` | §4: 23 recovered by two mechanical steps, and what stops the rest |

These are measurements of a history that only grows, so they are
scripts rather than tests: a test would fail every time a slice changed
the language, which is exactly the rate being measured.
