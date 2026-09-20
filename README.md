# lex-sys

A **systems dialect carrying Lex's philosophy**: native compilation, no GC, linear
ownership and capability-typed effects unified into one resource system, fully
defined behaviour, and a canonical content-addressable AST designed in from day one.

> **Status: M1 complete, M2 started, M3 started.** A bootstrap compiler takes a
> `.ls` file to a real native executable, and CI proves it on **linux-x86_64
> and darwin-aarch64**. The language has a type system: `int` and `bool`,
> structs, enums with exhaustive pattern matching, and monomorphised generics.
> Every declaration has a content hash (`lex-sys ids`).
>
> **Ownership works, in full.** A type is `res` or `val`, a `res` value is
> consumed exactly once on every path, and both borrow modes are in:
> `borrow x as &r` freezes for a read, `borrow mut x as &!r` locks for a
> write — all without a borrow checker, because a region is a block and the
> outlives relation is a stack. That is §3, §4 and §5 of the now-settled
> [`docs/linearity-and-effects.md`](docs/linearity-and-effects.md).
>
> **And the thesis holds.** Effect rows are exact, capabilities are ordinary
> linear values, and the two are one system: `[io]` on a signature means the
> function was handed an `&!i Io` it did not create. A function that is not
> given a capability cannot perform its effect — the whole safety story,
> stated as a type. That is §3 through §8 of the design document.
>
> **FFI is that story applied to C.** An `extern fn` declaration is reached
> only through an `Ffi("libc")` capability, its row names the library it
> calls into, and the two have to agree where the declaration is written.
> The capability comes from narrowing — `Ffi("")` can become `Ffi("libc")`
> and never the other way — so a program cannot grant itself what it was not
> given. C's effects stop being invisible at the point they enter.
>
> **Still missing:** arenas (§6). There are also no strings, no slices and
> no allocation: those are M3. Do not mistake this for a usable language
> yet.

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
cargo run -p lex-sys -- run examples/tour.ls
# M0: 7 5 3 1
# M1 bool: 1010010
# M1 struct: (3, 4) -> 25
# M1 enum: 0 12 20
# M1 generic: 5 3 z
# M2 linear: 4 7 9 5 6
# M2 borrow: 4 8 12
# M2 unique: 3 5 5
# M2 effects: 42
# M2 capability: 88
# M2 foreign: 7 9
```

`examples/tour.ls` is the shortest honest answer to "what can this language
do": one section per feature, in the order the milestones added them.
`examples/rational.ls` is a real 250-line program — exact rational arithmetic
with a generic `Result[T]` threaded through every fallible operation.

**What exists:** `int` and `bool`, functions and calls, arithmetic and
comparison, `&&`/`||` with short-circuiting, `if`/`else`, `while`, `let`/`var`
bindings, structs, enums with exhaustive `match`, generics over both,
`res`/`val` modes with exactly-once linearity and destructuring `let`,
shared and unique borrows with lexical regions, exact effect rows,
capabilities, narrowing, and capability-gated foreign calls.

```
res struct Ticket { serial: int }

fn redeem(t: Ticket) -> int {
    let Ticket { serial } = t;      // the whole is spent, the parts produced
    return serial;                  // `int` is `val`, so nothing is owed now
}
```

Mode is structural: a `res` member makes the whole aggregate `res`, and
`Held[File]` is `res` where `Held[int]` is `val`. There is no `drop` and no
destructor — a resource is destroyed by naming the function that knows how,
which is what keeps an effect row honest once there are effect rows.

Looking at a resource without spending it is a borrow:

```
fn serial_of[&r](t: &r Ticket) -> int { return t.serial; }

borrow held as &r in {
    putchar(48 + serial_of(r));   // `held` is frozen; `r` reads it
}                                 // owned again here
```

And writing to one is `borrow mut`, which *locks* rather than freezes —
nothing else may touch the value at all, not even a read, which is what makes
`&!r` the only way to reach it:

```
fn advance[&r](m: &!r Meter) -> int { m.reading = m.reading + m.step; return m.reading; }
```

Every signature declares what it does, and holds what lets it:

```
fn triple(n: int) -> [] int { return n * 3; }               // pure, and says so
fn show[&i](io: &!i Io, n: int) -> [io] int { ... }         // borrows the console

fn main(world: World) -> [] int {
    let Split { io, ffi } = split(world); // the one place authority comes from
    release(ffi);                         // unused authority is still a resource
    borrow mut io as &!i in { show(i, 7); }
    release(io);                          // a resource, destroyed exactly once
    return 0;
}
```

An effect **is** a borrowed capability. `[io]` on a signature means the
function was handed an `&!i Io` it did not create — so reading the row and
reading the parameter list are the same act, and a function that was given
nothing cannot print however much it wants to. There is no ambient
constructor: `Io { }` is refused, `main`'s `World` is the only authority in
the program, and it is linear, so forgetting to release it does not compile.

`main`'s own row is `[]` even though it prints, because it *owns* the
capability rather than borrowing one — and ownership is already visible in
the parameter list.

A row is a canonically ordered **set** — `[io, io]` is `[io]`, `[fs, io]` and
`[io, fs]` are one signature — so it hashes, which is what per-unit identity
is made of. It is exact in both directions: performing an effect you did not
declare and declaring one you never perform are both errors, because an
inexact row means `[]` stops meaning pure. And there is no list of legal
labels: every `io` traces back to a builtin that performs one, so a label
with nothing underneath it is refused the moment it is written.

A foreign call is the same idea pointed at C:

```
extern fn labs[&f](ffi: &f Ffi("libc"), n: int) -> [ffi("libc")] int;

let libc = narrow(ffi, "libc");        // `Ffi("")` -> `Ffi("libc")`, one way only
borrow libc as &f in { n = labs(f, 0 - 7); }
```

The declaration is the only place a foreign signature is written, the
capability is the only way to reach it, and the row names the library all
the way up every caller. The two have to agree where the declaration is
written: a foreign function that named an effect it holds no capability for,
or held one it did not declare, is refused there rather than at the call.
The capability itself is checked and then erased — what libc receives is the
integer and nothing else.

Narrowing is prefix extension, and it goes one way. `Ffi("")` names no
library at all, so it authorises nothing until it is narrowed; an
`Ffi("libcrypto")` can never become an `Ffi("libc")`. It also *consumes*
what it attenuates, so there is no way back to the wider capability — the
same commitment `lex-os` makes for manifests, for the same reason.

There is **no borrow checker**. A region is a block, so a reference's validity
is lexical rather than inferred: no non-lexical lifetimes, no variance, no
dataflow. A binding is `Owned`, `Frozen` or `Locked`, set at block entry and
restored at block exit; `r_inner <= r_outer` holds exactly when the outer
block encloses the inner one, which is a walk up a stack; and escape is an
occurs-check over one type.

**What does not, yet:** strings, slices and allocation — and arenas (§6),
the one part of the M2 document still unbuilt. That is why
`examples/hello.ls` still packs its greeting into two 64-bit words and
unpacks it a byte at a time, and why a foreign call can pass an integer but
not a pointer to bytes.

Every example declares what it prints in its own header, and a test walks
`examples/` and checks them, so an example that stops matching the language
fails CI rather than quietly rotting.

The compiler's whole surface:

```sh
lex-sys build <file.ls> [-o <output>] [--emit exe|obj]
lex-sys check <file.ls>     # refuse or say nothing
lex-sys run   <file.ls>     # build, run, exit with the program's status
lex-sys ids   <file.ls>     # each declaration's content hash
```

Exit codes are semantic: `0` success, `1` the program was refused with a
located diagnostic, `2` the command line was wrong, `3` the environment failed.

```sh
cargo test                                 # units, examples, and the conformance suite
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
```

### Layout

```
crates/lex-sys-syntax    lexer, canonical-shaped AST, parser
crates/lex-sys-types     the type vocabulary: representation and unification
crates/lex-sys-ir        resolution, type checking, monomorphisation; the IR
crates/lex-sys-codegen   Cranelift lowering, native object emission
crates/lex-sys-id        canonical encoding and content hashes
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
| [`docs/linearity-and-effects.md`](docs/linearity-and-effects.md) | The M2 gate: linear ownership, capability-typed effects, how they unify, and 23 must-reject fixtures written out as the conformance suite | settled; §3–5, §7 and §8 implemented |
| [`docs/bootstrap.md`](docs/bootstrap.md) | What M0 settled: bootstrap host (Rust), extension (`.ls`), the M0 surface, what is scaffolding and what replaces it | written |
| [`docs/canonical-ast.md`](docs/canonical-ast.md) | Canonicalisation rules and per-unit identity: what is hashed, and what a hash is allowed to change with | written, implemented |
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
| **M1** — typed core | Type checker, `bool`, structs, ADTs with exhaustiveness, monomorphised generics. No linearity, no effects — deliberately | **done** |
| **M2** — the actual thesis ([#2](https://github.com/alpibrusl/lex-sys/issues/2)) | Linear ownership, effect rows and capability-passing as **one** system | **done** — §3–§5, §7 and §8, narrowing and FFI included; arenas (§6) remain |
| **M3** — minimal but real | Slices and strings, arenas, libc FFI, settled overflow semantics, canonical printer, per-unit identity | started — per-unit identity landed, and libc FFI arrived early with M2's capabilities |

Deliberately excluded from "minimal": borrow checker, traits, `comptime`, own
optimiser, incremental compilation, LSP, async. Each is "yes, later" — saying
yes early is what turns three months into three years.

Beyond M3 the first real target is **`lex-os`** — production systems work, no rewrite
risk. Self-hosting the lex-lang toolchain stays a *spike before a plan*: port
`lex-ast`/`lex-vcs` canonical forms and verify byte-identical `OpId`/`SigId`/`StageId`
over the existing ~136k-op corpus, then decide.

## Licence

[EUPL-1.2](LICENSE), matching the rest of the ecosystem. See `LICENSE` for the
notice and where to obtain the full text in any of the 23 EU languages.
