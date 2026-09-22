# Related work

> **Status: written, and overdue.**
>
> Nothing in lex-sys is a new idea on its own. Linear types, regions,
> effect rows and capabilities each have decades of prior work, and at
> least one language — Austral — reached most of this project's core
> before it did. Until this document, the repository credited four of the
> projects it drew on in passing and **did not mention Austral at all**.
> An outside audit pointed that out, and it was right.
>
> Three parts: what was **taken**, traced to the document that took it;
> the **closest relatives**, and what differs; and the **competitor** for
> the use case this project is actually for. Where lex-sys differs, this
> says so in one line, because the differences are the only part a reader
> could not have found elsewhere.

---

## 1. What was taken

| From | What | Where it landed |
|---|---|---|
| **Lex** | Effects in the type system, effect declarations as a function's contract, examples as tests, content-addressed identity | The whole project — [`linearity-and-effects.md`](linearity-and-effects.md), [`canonical-ast.md`](canonical-ast.md) |
| **Cyclone** | Regions as lexical scopes, with the checker confirming nothing escapes its block | [`linearity-and-effects.md`](linearity-and-effects.md) §5 says it plainly: *"Cyclone's idea with Rust's inference deleted"* |
| **Koka** | Effects as a **row** — a set of labels on the arrow, not a monad in the return type | [`linearity-and-effects.md`](linearity-and-effects.md) §2 and §7. lex-sys keeps the row and drops the rest: no handlers, and no row polymorphism ([`effect-polymorphism.md`](effect-polymorphism.md)) |
| **Rust** | Ownership as *move*, `&` and `&mut`-shaped borrows, monomorphised generics | Throughout. What it declines is the borrow checker: lifetimes are lexical here, never inferred ([`aliasing.md`](aliasing.md)) |
| **Vale** | **Generational references** — a handle that is a plain index plus a generation, checked on every use against the arena that issued it | [`sharing.md`](sharing.md) §3's `Gen`, which is this idea built as a library. That document now says so |
| **Zig** | `defer`, and the case against hidden control flow | [`defer.md`](defer.md), including why Zig's `errdefer` is refused here |
| **The object-capability model** — E, and Mark Miller's *Robust Composition* | No ambient authority: a function can do only what it was handed, and authority is a value that can be attenuated but not forged | `World`, `split`, and one-way `narrow` — [`authority.md`](authority.md) |
| **Capsicum** | That a Unix process can drop into a mode where it holds only the descriptors it was given | The same instinct at the language level: `release` destroys authority a program will not use, and nothing re-creates it |

---

## 2. The closest relatives

These are the projects a reader should compare lex-sys with, and in
several cases they got there first.

**Austral** is the nearest. Linear types; capabilities as linear values,
with a root capability handed to the entry point exactly as lex-sys hands
`main` a `World`; borrowing through lexical regions; no borrow checker in
Rust's sense; and a specification deliberately small enough to hold in
one's head. Most of lex-sys's core is in Austral, and Austral had it
first. *What lex-sys adds:* the authority a function holds is also stated
as an **exact effect row**, checked in both directions, which is what lets
`lex-sys authority` answer *what can this program reach* without reading
any body — and a canonical, content-addressed AST underneath.

**Effekt** treats **effects as capabilities**: an effect handler is passed
to the code that uses it, and is second-class so it cannot escape its
scope. That is the effects-literature form of this project's rule that
*an effect is a borrowed capability*, arrived at from the other direction.
*What differs:* Effekt's capabilities are handlers, with the control-flow
power that implies; lex-sys has no handlers, and its capabilities stand for
authority over the outside world rather than for control effects.

**Pony** has **reference capabilities** — `iso`, `val`, `ref` and others —
that make data races a type error in an actor model. *What differs:* the
problem, mostly. Pony's capabilities govern aliasing between concurrent
actors; lex-sys's govern authority, and it has no concurrency yet. The two
share the word `val` with **different meanings** — in Pony, deeply
immutable and shareable; here, copyable and discardable — which is worth
knowing before reading either.

**Hylo** (formerly Val) gets memory safety from **mutable value
semantics**, with references second-class so they cannot be stored.
*What differs:* lex-sys takes the other road — references are ordinary
values, and a lexical region is what bounds them.

**Linear Haskell, ATS, Mezzo and Vault** are the linear- and
typestate-types lineage these languages share, and anyone asking where
`res` and `val` come from should start there.

---

## 3. The competitor

For this project's actual use case — running code you did not write, with
authority you chose — the incumbent is not Rust or C. It is
**WebAssembly with WASI**: any source language, capability-style handles,
and a sandbox enforced by the runtime.

lex-sys has to win on what that design cannot do, and it is one axis:

| | WASI | lex-sys |
|---|---|---|
| **When authority is known** | At run time, by trying | **Before execution**, from the program's text |
| Granularity | Preopened directories and sockets | A path prefix today; a host per [`net.md`](net.md) |
| Runtime cost | A Wasm runtime and its sandbox | None — a native binary, the checks erased at compile time |
| Language | Any | lex-sys only |

The honest summary is the audit's: *know what a native binary can touch
before you run it, at no runtime cost.* That is a narrower claim than
WASI makes, and a stronger one where it applies — and under `lex-os` it
is **defence in depth** rather than a replacement: a static proof before
load, and a supervisor while it runs.

It is also a claim with a hole in it today, stated where it matters:
[`under-a-grant.md`](under-a-grant.md) shows that a program reaching
`Ffi("libc")` can do anything libc can, and the report now says so by
failing closed.

---

## 4. What this does not claim

* **Not a survey.** These are the projects a reader of this repository
  is likely to know or should know, not the literature.
* **Not that lex-sys is better than any of them.** Austral is further
  along as a language, Koka and Effekt as effect systems, Pony at
  concurrency, WASI at deployment. The claim is only that the
  *combination* — linear ownership, capability effects, an exact row and
  a native binary — is the one this project is testing.
