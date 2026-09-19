# lex-sys

A **systems dialect carrying Lex's philosophy**: native compilation, no GC, linear
ownership and capability-typed effects unified into one resource system, fully
defined behaviour, and a canonical content-addressable AST designed in from day one.

> **Status: M0 complete.** A bootstrap compiler takes a `.ls` file to a real
> native executable — lexer, parser, canonical-shaped AST, resolved IR,
> Cranelift backend — and CI proves it on **linux-x86_64 and darwin-aarch64**,
> which is M0's acceptance criterion verbatim. The M2 design gate,
> [`docs/linearity-and-effects.md`](docs/linearity-and-effects.md), is written
> and awaiting review.
>
> There is **no type system, no linearity and no effects yet** — those are M1
> and M2. The whole language today is `int`, functions, arithmetic,
> comparison, `if`/`else`, `while` and `let`/`var`. Do not mistake it for a
> usable one.

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

## Try it

```sh
cargo run -p lex-sys -- build examples/hello.ls -o hello && ./hello
# Hello, world!
```

The M0 language is one type (`int`), functions and calls, arithmetic,
comparison, `if`/`else`, `while`, and `let`/`var` bindings — that is all of it.
`examples/hello.ls` prints its greeting by unpacking two 64-bit words a byte at
a time, because there are no strings yet, and that is honest about where the
milestone ends.

The compiler's whole surface:

```sh
lex-sys build <file.ls> [-o <output>] [--emit exe|obj]
lex-sys check <file.ls>     # refuse or say nothing
lex-sys run   <file.ls>     # build, run, exit with the program's status
```

Exit codes are semantic: `0` success, `1` the program was refused with a
located diagnostic, `2` the command line was wrong, `3` the environment failed.

```sh
cargo test                                 # 53 tests: units + the conformance suite
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
```

### Layout

```
crates/lex-sys-syntax    lexer, canonical-shaped AST, parser
crates/lex-sys-ir        resolution and every check the compiler makes; the M0 IR
crates/lex-sys-codegen   Cranelift lowering, native object emission
crates/lex-sys           the CLI
examples/                programs meant to be read
tests/accept             fixtures that must compile and run
tests/reject             fixtures that must be refused, each stating why
docs/                    design documents
```

Everything a program can be refused for is refused in `lex-sys-ir`, so the
backend has no error path for a *program* — only for the environment.

Each fixture declares its own expectation in its header (`//~ STDOUT`,
`//~ EXIT`, `//~ ERROR`), so adding a rule to the language means adding a
fixture. M2 requires a must-reject fixture per rule; the harness that will
carry them exists now, while it is cheap.

## Design documents

| Doc | What | Status |
|---|---|---|
| [`docs/linearity-and-effects.md`](docs/linearity-and-effects.md) | The M2 gate: linear ownership, capability-typed effects, how they unify, and 23 must-reject fixtures written out as the conformance suite | written, awaiting review |
| [`docs/bootstrap.md`](docs/bootstrap.md) | What M0 settled: bootstrap host (Rust), extension (`.ls`), the M0 surface, what is scaffolding and what replaces it | written |
| `docs/canonical-ast.md` | Canonicalisation rules and per-unit identity | not written (M3) |
| `docs/memory-model.md` | Regions, escape, the escape hatches and their cost | not written (M2) |
| `docs/defined-behaviour.md` | Every place C and Rust leave behaviour open, and what we define it to | not written (M3) |

Division already traps on a zero divisor and on `int::MIN / -1` rather than
being undefined, with a fixture that runs the trap and asserts the process dies
rather than continuing with nonsense. That is the difference the language exists
to make, and it is in from the first milestone.

## Roadmap

Tracked in the epic: **[#1](https://github.com/alpibrusl/lex-sys/issues/1)** — milestones
M0–M3 with acceptance criteria, sequencing, risks and open decisions.

| Milestone | What | Status |
|---|---|---|
| **M0** — native hello world ([#3](https://github.com/alpibrusl/lex-sys/issues/3)) | Lexer, parser, AST, IR, Cranelift backend, a real executable | **done** — green on both targets |
| **M1** — typed core | Bidirectional checker, structs, ADTs with exhaustiveness, monomorphised generics. No linearity, no effects — deliberately | next |
| **M2** — the actual thesis ([#2](https://github.com/alpibrusl/lex-sys/issues/2)) | Linear ownership, effect rows and capability-passing as **one** system | **gated** on `docs/linearity-and-effects.md`, written and awaiting review |
| **M3** — minimal but real | Slices and strings, arenas, libc FFI, settled overflow semantics, canonical printer, per-unit identity | later |

Deliberately excluded from "minimal": borrow checker, traits, `comptime`, own
optimiser, incremental compilation, LSP, async. Each is "yes, later" — saying
yes early is what turns three months into three years.

Beyond M3 the first real target is **`lex-os`** — production systems work, no rewrite
risk. Self-hosting the lex-lang toolchain stays a *spike before a plan*: port
`lex-ast`/`lex-vcs` canonical forms and verify byte-identical `OpId`/`SigId`/`StageId`
over the existing ~136k-op corpus, then decide.

## Licence

To be decided (the org default is EUPL-1.2).
