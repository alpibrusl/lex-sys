# lex-sys

A **systems dialect carrying Lex's philosophy**: native compilation, no GC, linear
ownership and capability-typed effects unified into one resource system, fully
defined behaviour, and a canonical content-addressable AST designed in from day one.

> **Status: design stage.** Nothing is implemented. This repo currently holds the
> design work and the epic tracking it. Do not expect a compiler here yet.

## What this is

`lex-sys` is **not** Lex, and **not** a subset of it. Lex is the high-level,
functional, GC'd, interpreted language the ecosystem's libraries are written in.
`lex-sys` would be a *second, lower-level language* sharing Lex's worldview —
effects and capabilities in the type system, determinism as a first-class
property — but targeting the work Lex can't do: native binaries, manual and
region memory, syscalls, embedding, FFI.

The two are designed to interoperate over C FFI, with Lex remaining the
application layer.

## Why

Three things Lex's philosophy buys that no systems language currently combines:

1. **Capabilities all the way down.** Ownership and effects are the same idea —
   both are resource tracking. A from-scratch language can unify them: allocation
   is an effect, a heap value is a linear resource, FFI is a capability you must
   be granted.
2. **Determinism as a language property.** No UB, defined evaluation order,
   deterministic layout. This is what makes replay, attestation and
   content-addressing mean anything — and it is exactly what C throws away.
3. **A checker that is fast and total,** because the guarantee is only worth what
   it costs to verify.

## Design commitments

| Area | Commitment | Why |
|---|---|---|
| Memory | Linear/affine types + regions/arenas — **not** an NLL borrow checker | Local, cheap, total to check |
| Effects | Capability-typed effects in the type system, unified with linearity | One resource system, not two |
| Behaviour | No UB, defined evaluation order, deterministic layout | Reproducibility is load-bearing |
| AST | Canonical, content-addressable, stable per-unit identity | Designed in, never retrofitted |
| Types | Fast, total, decidable inference | The guarantee must be cheap |
| Metaprogramming | Hygienic deterministic `comptime` — **no** textual or proc macros | Macros break stable content-addressing |
| Generics | Monomorphised | Zero-cost, matches Rust's codegen |
| FFI | Explicit, capability-gated | C's effects must not be invisible |
| Backend | Cranelift for dev, LLVM for release | Reuse; own backend only if zero-C becomes a goal |

Carried over from Lex: examples-as-tests, `[budget]`, effect declarations as the
function's contract.

## Explicit non-goals

- **Not a Rust clone.** No trait-system maximalism, no GATs, no specialisation, no
  borrow checker. Chasing Rust's *power* means inheriting Rust's implementation
  cost and abandoning totality — that is the failure mode this design exists to
  avoid.
- **Not self-hosting-first.** Porting the lex-lang toolchain is a possible
  end-state, not a starting point.
- **Not a replacement for Lex.** Different layer, different job.

## Performance expectation

The ceiling is Rust's. Linearity and effects are erased at compile time; generics
monomorphise; an LLVM backend inherits rustc's own optimiser. Linearity can hand
the optimiser *stronger* aliasing facts than `&mut` does, and known purity enables
reordering Rust can't justify. The one structural cost is defining away UB —
principally integer-overflow semantics — worth a low single-digit percent.

Any larger gap early on is implementation maturity, not language design.

## Roadmap

See the tracking epic for milestones, acceptance criteria and sequencing.

## Licence

To be decided (the org default is EUPL-1.2).
