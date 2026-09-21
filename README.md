# lex-sys

A **systems dialect carrying Lex's philosophy**: native compilation, no GC,
linear ownership and capability-typed effects unified into one resource
system, fully defined behaviour, and a canonical content-addressable AST
designed in from day one.

> **Status: M0–M3 complete**, and post-milestone work is shipping one slice
> at a time. A bootstrap compiler takes a `.ls` file to a real native
> executable, and CI proves it on **linux-x86_64 and darwin-aarch64**.
>
> Not a usable language yet. [`docs/ROADMAP.md`](docs/ROADMAP.md) tracks
> what landed, what is next, and what each slice found.

---

## What this is

`lex-sys` is **not** Lex, and **not** a subset of it. Lex is the high-level,
functional, GC'd, interpreted language the ecosystem's libraries are written
in. `lex-sys` is a *second, lower-level language* sharing Lex's worldview —
effects and capabilities in the type system, determinism as a first-class
property — targeting the work Lex can't do: native binaries, manual and
region memory, syscalls, embedding, FFI.

The two are designed to interoperate over C FFI, with Lex remaining the
application layer.

## Why

Three things Lex's philosophy buys that no systems language currently
combines:

1. **Capabilities all the way down.** Ownership and effects are the same
   idea — both are resource tracking. A from-scratch language can unify
   them: allocation is an effect, a heap value is a linear resource, FFI is
   a capability you must be granted.
2. **Determinism as a language property.** No UB, defined evaluation order,
   deterministic layout. This is what makes replay, attestation and
   content-addressing mean anything — and it is exactly what C throws away.
   Written out operation by operation in
   [`docs/defined-behaviour.md`](docs/defined-behaviour.md).
3. **A checker that is fast and total,** because the guarantee is only worth
   what it costs to verify.

---

## The language in one page

**A value is `res` or `val`.** A `res` value is consumed exactly once on
every path. Mode is structural — a `res` member makes the whole aggregate
`res` — so `Held[File]` is `res` where `Held[int]` is `val`.

```
res struct Ticket { serial: int }

fn redeem(t: Ticket) -> [] int {
    let Ticket { serial } = t;      // the whole is spent, the parts produced
    return serial;                  // `int` is `val`, so nothing is owed now
}
```

There is no `drop` and no destructor: a resource is destroyed by naming the
function that knows how, which is what keeps an effect row honest once there
are effect rows.

**Looking without spending is a borrow.** `borrow` freezes for a read,
`borrow mut` *locks* for a write — nothing else may touch the value at all,
not even a read.

```
fn serial_of[&r](t: &r Ticket) -> [] int { return t.serial; }

borrow held as &r in {
    putchar(i, 48 + serial_of(r));    // `held` is frozen; `r` reads it
}                                     // owned again here
```

There is **no borrow checker**. A region is a block, so a reference's
validity is lexical rather than inferred: no non-lexical lifetimes, no
variance, no dataflow. A binding is `Owned`, `Frozen` or `Locked`, set at
block entry and restored at block exit; `r_inner <= r_outer` holds exactly
when the outer block encloses the inner one, which is a walk up a stack; and
escape is an occurs-check over one type.

**An effect *is* a borrowed capability.**

```
fn triple(n: int) -> [] int { return n * 3; }                // pure, and says so
fn show[&i](io: &!i Io, n: int) -> [io_write] int { ... }    // borrows the console

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);    // the one source of authority
    release(args); release(heap); release(fs); release(ffi); // unused authority is still a resource
    borrow mut io as &!i in { show(i, 7); }
    release(io);                                             // destroyed exactly once
    return 0;
}
```

`[io_write]` on a signature means the function was handed an `&!i Io` it did
not create — so reading the row and reading the parameter list are the same
act, and a function that was given nothing cannot print however much it
wants to. There is no ambient constructor: `Io { }` is refused, `main`'s
`World` is the only authority in the program, and it is linear, so forgetting
to release it does not compile.

`main`'s own row is `[]` even though it prints, because it *owns* the
capability rather than borrowing one — and ownership is already visible in
the parameter list.

A row is a canonically ordered **set**, so it hashes, which is what per-unit
identity is made of. It is exact in both directions: performing an effect you
did not declare and declaring one you never perform are both errors, because
an inexact row means `[]` stops meaning pure. And there is no list of legal
labels — every label traces back to a builtin that performs one, so a label
with nothing underneath it is refused the moment it is written.

**A foreign call is the same idea pointed at C.**

```
extern fn labs[&f](ffi: &f Ffi("libc"), n: int) -> [ffi("libc")] int;

let libc = narrow(ffi, "libc");        // `Ffi("")` -> `Ffi("libc")`, one way only
borrow libc as &f in { n = labs(f, 0 - 7); }
```

Narrowing is prefix extension and goes one way. `Ffi("")` names no library,
so it authorises nothing until narrowed; an `Ffi("libcrypto")` can never
become an `Ffi("libc")`. It also *consumes* what it attenuates, so there is
no way back to the wider capability — the same commitment `lex-os` makes for
manifests, for the same reason. The capability is checked and then erased:
what libc receives is the integer and nothing else.

**Memory has three shapes**, and the same region machinery checks all of
them.

```
region a {                                   // an arena: released in one call
    let xs = alloc_slice[a](5, 0);           // xs : &!a [int]
    xs[0] = 3;                               // bounds-checked; out of range traps
}

let node = box(h, Node { value: 1 });        // the heap: `Box[T]` is `res`
let tail = unbox(h, node);                   // the only thing that ends one
```

An arena *is* a region — same block, same parent chain, same occurs-check.
Nothing whose type mentions `a` leaves the block, and release is one `free`
whatever was allocated. Arenas and boxed slices hold `val` data only:
releasing one reclaims memory and **runs nothing**, so a linear value inside
would have its obligation dropped rather than discharged.

`Box[T]` is `res`, so the exactly-once rule written for capabilities and
file handles turns out to be a *memory safety* rule for free: no leaks, no
double frees, no use-after-free, none of them checked by anything new. A
type may contain itself through a `Box`, which is what makes linked
structures compile.

**A string is a run of bytes and claims no encoding.** `str` is not a type;
a string is `&r [byte]`, an ordinary slice and therefore an ordinary
reference, so regions, the escape check and the unique-to-shared coercion
all came for free. `byte` is storage rather than arithmetic — `byte_of` and
`int_of` convert, and `byte_of` traps rather than truncating.

**A reference gives references.** Matching `&l List` binds every payload as
a reference into the list, carrying the scrutinee's mode and region, and
`*r` reads a `val` referent. Nothing moves out of a reference, so a `res`
payload binds as a borrow and linearity is untouched.

**Arithmetic is checked.** `+`, `-`, `*` and unary `-` produce the right
answer or **trap** — they never wrap. Wrapping is expressible but has to be
asked for by name. Division traps on a zero divisor and on `int::MIN / -1`
rather than being undefined. Evaluation order is left to right everywhere,
including a struct literal's fields, and that is enforced rather than merely
intended.

---

## What exists

`int`, `byte` and `bool`; functions and calls; arithmetic and comparison;
`&&`/`||` with short-circuiting; the bit operators and hexadecimal
literals; `if`/`else`, `while`, `let`/`var`; structs,
enums with exhaustive `match`, tuples, and generics over all of them with
`[T: val]` mode bounds; `res`/`val` linearity with destructuring `let` and
liveness-checked shadowing; shared and unique borrows with lexical regions;
exact effect rows, capabilities, narrowing, and capability-gated foreign
calls; arenas, a general heap with recursive types, boxed slices and the
growable buffers they allow; slices and strings; file IO through a
path-carrying capability; the console in both directions; the command line;
programs spread over several files; modules with visibility; and a standard
library of eight modules.

What is **not** there yet, and why, is [`docs/ROADMAP.md`](docs/ROADMAP.md).

### What that adds up to

A REST endpoint, for one — `examples/serve/` binds a TCP port, accepts a
connection, routes the request and answers with JSON, and the test suite
makes the request over a real socket:

```sh
$ ./serve 8080 &
$ curl -s http://127.0.0.1:8080/health
{"ok":true}
```

There is no socket type, no `Net` capability and no HTTP library. Sockets
are libc, libc has a name, and the capability that names it has existed
since M2 — **what decides whether a program is writable here is not a
feature list, it is whether the authority it needs has a name.**

The same rule says what is out of reach, and it is one sentence: a foreign
*result* is a scalar, so anything that hands back an opaque pointer — TLS,
libpq, `FILE *`, `dlopen` — is not reachable yet, because a pointer from C
carries no region and this language has no reference that lacks one.
Threads are out for a different reason: `pthread_create` wants a function
pointer, and there are no function values; `fork` returns an `int`, so
several processes are fine.

[`docs/reach.md`](docs/reach.md) is the measured version, including the
place where narrowing runs out: the row says `ffi("libc")` and cannot say
`net`, because a library is not an authority domain.

And one program here did not start here. `examples/base64/` is GNU
coreutils' `base64`, ported and checked byte-for-byte against it in both
directions. It needed the bit operators, which did not exist, and nothing
else — no new capability, no library, no change to linearity or effect
rows. Its authority report is three lines and every one is checkable from
outside: `args`, `io_read`, `io_write`, and it never touches the
filesystem, the heap or foreign code.
[`docs/porting.md`](docs/porting.md), including §6 on what one small port
does not establish.

---

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

Carried over from Lex: examples-as-tests, `[budget]`, effect declarations as
the function's contract.

## Explicit non-goals

- **Not a Rust clone.** No trait-system maximalism, no GATs, no
  specialisation, no borrow checker. Chasing Rust's *power* means inheriting
  Rust's implementation cost and abandoning totality — the failure mode this
  design exists to avoid.
- **Not self-hosting-first.** Porting the lex-lang toolchain is a possible
  end-state, not a starting point.
- **Not a replacement for Lex.** Different layer, different job.

## Performance expectation

Linearity and effects are erased at compile time; generics monomorphise; an
LLVM backend inherits rustc's own optimiser. Linearity can hand the optimiser
*stronger* aliasing facts than `&mut` does, and known purity enables
reordering Rust can't justify.

The one structural cost is defining away UB — principally integer-overflow
semantics — and it has now been **measured** rather than estimated
([`docs/overflow-cost.md`](docs/overflow-cost.md)):

| what dominates the loop | checked arithmetic costs |
|---|---|
| calls and returns | +2.8% |
| memory and cache | +3.6% |
| comparisons and branches | −9.1% (checked was *faster*) |
| arithmetic, nothing else | **+40.5%** |

So it is not a percentage, it is a rule: **the cost is whether arithmetic is
on the critical path.** For most systems code it is not, and the price is
the low single digits this file used to promise across the board. For a
tight reduction it is large, and the reason is not the never-taken branch —
it is that a trap is observable, so the loop cannot vectorise. Measured in C
at `-O2`, the same guarantee costs clang 46% and gcc 74% on the same shape,
which is how we know it is the semantics and not the young backend.

This is where "the ceiling is Rust's" needs a qualifier, and the qualifier
is real: Rust's release profile *wraps*, so on arithmetic-bound code the
ceiling is Rust's only if you are comparing against a Rust build that also
checks. Everywhere else the claim stands, and any larger gap early on is
implementation maturity rather than language design.

---

## Try it

```sh
cargo run -p lex-sys -- run examples/tour.ls
```

[`examples/`](examples/README.md) is a guided index: what each program is,
what it prints, and which one to read for which idea. Every example declares
its own output in its header, and a test walks the directory and checks
them, so an example that stops matching the language fails CI rather than
quietly rotting.

### The compiler's whole surface

```sh
lex-sys build <file.ls>... [-o <output>] [--emit exe|obj] [--std]
lex-sys check <file.ls>... [--std]    # refuse, or say nothing
lex-sys run   <file.ls>... [--std]    # build, run, exit with the program's status
lex-sys ids   <file.ls>... [--std]    # each declaration's content hash
lex-sys authority <file.ls>... [--std] [--output json]  # what it can reach
lex-sys print <file.ls>               # the unit, rendered in canonical form
```

`authority` is the one worth trying on something you did not write:

```sh
$ lex-sys authority examples/tally.ls --std
performs
    io_read
    io_write
never touches
    the filesystem
    the heap
    the command line
    foreign code
```

The surface is the union of what everything `main` reaches performs, so
it is precise rather than conservative — rows are exact in both
directions. An absent label is a proof: the capability was released, and
nothing in the language creates another. `--output json` gives the same
report as data, for a supervisor checking it against a grant.
[`docs/authority.md`](docs/authority.md).

A program is the **set of files named on the command line**, in any order.
Each is in a module — the root, unless it says `module a.b;` — and reaches
another module's names through `import`.

Exit codes are semantic: `0` success, `1` the program was refused with a
located diagnostic, `2` the command line was wrong, `3` the environment
failed.

`--std` makes the standard library's source available. It is compiled into
the binary rather than looked up, so it adds no search path and no manifest,
and it is never a prelude: a program still writes `import std.io;` where it
uses one.

### The gate

```sh
cargo test                                 # units, examples, and the conformance suite
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
```

---

## Layout

```
crates/lex-sys-syntax    lexer, canonical-shaped AST, parser
crates/lex-sys-types     the type vocabulary: representation and unification
crates/lex-sys-ir        resolution, type checking, monomorphisation; the IR
crates/lex-sys-codegen   Cranelift lowering, native object emission
crates/lex-sys-id        canonical encoding and content hashes
crates/lex-sys           the CLI
std/                     the standard library, as Lex source
examples/                programs meant to be read
tests/accept             fixtures that must compile and run
tests/reject             fixtures that must be refused, each stating why
benches/                 checked/wrapping pairs; what the overflow trap costs
scripts/bench.py         runs them and prints the table
docs/                    design documents
```

Everything a program can be refused for is refused in `lex-sys-ir`, so the
backend has no error path for a *program* — only for the environment.

Each fixture declares its own expectation in its header (`//~ STDOUT`,
`//~ EXIT`, `//~ ERROR`), so adding a rule to the language means adding a
fixture.

---

## Documentation

| Where | What |
|---|---|
| [`docs/README.md`](docs/README.md) | Every design document, what it settles, and whether it is built |
| [`docs/ROADMAP.md`](docs/ROADMAP.md) | Milestones, what each slice landed and found, and what is next |
| [`examples/README.md`](examples/README.md) | The programs, and which to read for which idea |

Design lands in `docs/` **before** the code that implements it, which is the
cheap place for it to be wrong. When it turns out wrong anyway, the document
that made the claim is corrected in place rather than quietly edited — four
of them carry a correction now, and the roadmap says which.

## Licence

[EUPL-1.2](LICENSE), matching the rest of the ecosystem. See `LICENSE` for
the notice and where to obtain the full text in any of the 23 EU languages.
