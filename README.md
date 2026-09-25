# lex-sys

A **native systems language in which resource ownership and authority are
part of the program's type-level contract.** Linear ownership and
capability-typed effects are one system rather than two; behaviour is
fully defined, with no UB; and the AST is canonical and
content-addressable, designed in from day one rather than retrofitted.

> **Status.** M0–M3 complete, and post-milestone work ships one slice at a
> time. A bootstrap compiler takes a `.ls` file to a real native
> executable, green on **linux-x86_64 and darwin-aarch64**.
>
> **What works today:** the thesis, built and enforced —
> capabilities, exact effect rows, one-way narrowing, lexical borrowing,
> arenas and a general heap, file handles, the console in three
> directions, libc FFI, compile-time evaluation, a standard library
> written in lex-sys, a `Net` capability with outbound `connect` and
> inbound `bind`/`listen`/`accept` (edition 2, [`docs/net.md`](docs/net.md)),
> and `lex-sys authority`, which computes what a program can reach from
> the same reachability that decides what goes in the binary. Real
> programs: GNU `base64` and `sort` ported and checked byte-for-byte
> against the originals, and a REST endpoint answered over a real socket.
>
> **What does not:** it is **not a usable language yet** — no threads, no
> TLS, and a **1.17×–2.58×** gap to C that is a property of the backend
> rather than of the design ([below](#performance-honestly)).
> [`docs/ROADMAP.md`](docs/ROADMAP.md) tracks what landed, what is next,
> and what each slice found.

> **Writing lex-sys?** [`AGENTS.md`](AGENTS.md) is the one page — the
> rules, the six things that cost this repository a compile each, and
> what the language does not have. `lex-sys agent-guidelines` prints it,
> and every checked code block in it is run by the test suite.

---

## Where this sits

Three repositories share one idea. **Two of them are wired together, and
this is the third.**

```
                    lex-lang
             the high-level language
       16 crates: syntax, ast, types, store,
          vcs, jit, lsp, bytecode, trace
                       │
                       │  Grant, and the real Lex front end
                       ▼
                    lex-os
           the autonomous-agent runtime
     manifest + grant → static check → perimeter
            → supervisor → audit chain


                    lex-sys
                    this repo
       a second, native language, same worldview
```

**`lex-lang`** is the high-level, functional, GC'd, interpreted language
the ecosystem's libraries are written in.

**`lex-os`** is the runtime that takes an agent's goal, seals it in a
microVM, and mediates everything it does against **one** declaration —
the trust `Grant`. That grant is enforced twice: statically, by
`lex-os-check`, which rejects a program whose effects exceed it *before
it loads*; and at run time, by a supervisor the agent cannot reach,
which logs every request to a hash-chained audit file outside the box
before deciding it. Its demo runs an agent as root, lets it attempt
three escapes, and stops each with a different mechanism.

**`lex-sys`** — this repository — is a *second, lower-level language*
sharing that worldview, targeting the work Lex cannot do: native
binaries, manual and region memory, syscalls, embedding, FFI. It shares
the idea and **no code**: `lex-os` takes its grant from `lex-lang` and
checks `.lex`, and does not depend on this repository at all. Two joins
are named and both are gated —

| Join | State |
|---|---|
| lex-sys code in `lex-vcs` | 81% of that crate is already language-agnostic; gated on a **plateau** in the effect vocabulary rather than on a feature — [`hash-stability.md`](docs/hash-stability.md) |
| lex-sys code under a lex-os grant | Not a compiler integration: `authority --output json` is already the right interface, and the grant's **filesystem** dimension works through it today. The vocabulary now has `network` too — `net_out`/`net_in`, [`net.md`](docs/net.md) — but no program in this repository has been ported onto it yet, so today's reports still say `ffi("libc")`; `exec` remains invisible either way — [`under-a-grant.md`](docs/under-a-grant.md) |

Saying so plainly is deliberate: a reader of an earlier version of this
page could not tell that `lex-os` existed at all, which is what
[`docs/first-page.md`](docs/first-page.md) measures.

---

## Why

Three things this philosophy buys that no systems language currently
combines:

1. **Capabilities all the way down.** Ownership and effects are the same
   idea — both are resource tracking. Allocation is an effect, a heap
   value is a linear resource, FFI is a capability you must be granted.
   Owning a capability *discharges* its effects and borrowing *declares*
   them, so `main`'s row is `[]` however much it does.
2. **Determinism as a language property.** No UB, defined evaluation
   order, deterministic layout — which is what makes replay, attestation
   and content-addressing mean anything, and exactly what C throws away.
   Written out operation by operation in
   [`docs/defined-behaviour.md`](docs/defined-behaviour.md).
3. **A checker that is fast and total,** because the guarantee is only
   worth what it costs to verify.

---

## What exists

| | | Settled by |
|---|---|---|
| **Capabilities** | `World`, `Io`, `Fs(prefix)`, `Ffi(lib)`, `Heap`, `Args`, `File`, `Net(bound)` — linear values, `split` once, released by name | [`linearity-and-effects.md`](docs/linearity-and-effects.md) |
| **Effect rows** | A canonically ordered set, exact in both directions, every label tracing to a builtin | [`linearity-and-effects.md`](docs/linearity-and-effects.md) |
| **Narrowing** | Prefix extension, one way, and it *consumes* what it attenuates | [`filesystem.md`](docs/filesystem.md), [`reach.md`](docs/reach.md) |
| **Authority report** | `lex-sys authority`, computed from reachability; `--output json` for a supervisor, and it **fails closed** on foreign code | [`authority.md`](docs/authority.md) |
| **Borrowing** | Lexical regions, no borrow checker; `&!` is a lock on the binding, answered with a measurement | [`aliasing.md`](docs/aliasing.md) |
| **Memory** | Arenas, a general heap with recursive types, boxed slices, growable buffers | [`heap.md`](docs/heap.md), [`boxed-slices.md`](docs/boxed-slices.md) |
| **Types** | `int` `byte` `bool` `float`, structs, enums with exhaustive `match`, tuples, generics with `[T: val]` bounds | [`floating-point.md`](docs/floating-point.md), [`tuples.md`](docs/tuples.md) |
| **Defined behaviour** | Checked arithmetic that traps, left-to-right evaluation, every C hole named and closed | [`defined-behaviour.md`](docs/defined-behaviour.md) |
| **Program identity** | `lex-sys ids` — per-declaration content hashes, 35 golden fixtures | [`canonical-ast.md`](docs/canonical-ast.md), [`hash-stability.md`](docs/hash-stability.md) |
| **Compile time** | Pure calls on constant arguments folded; `static` items whose bodies run during compilation | [`compile-time-data.md`](docs/compile-time-data.md) |
| **I/O** | The console in three directions, file handles as linear resources, bulk reads and writes | [`file-handles.md`](docs/file-handles.md), [`bulk-io.md`](docs/bulk-io.md) |
| **Refusals** | 53 rules, each with a stable tag; `check --output json` reports every independent one | [`agent-errors.md`](docs/agent-errors.md) |
| **Standard library** | 11 modules written in lex-sys, including shortest round-trip float printing and a UTF-8 decoder | [`standard-library.md`](docs/standard-library.md) |

What is **not** there yet, and why, is [`docs/ROADMAP.md`](docs/ROADMAP.md).

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


---

### What that adds up to

A REST endpoint, for one — `examples/serve/` binds a TCP port, accepts a
connection, routes the request and answers with JSON, and the test suite
makes the request over a real socket:

```sh
$ ./serve 8080 &
$ ./fetch 127.0.0.1 8080 /health
{"ok":true}
```

Both ends of that exchange are lex-sys: `examples/fetch/` is the client,
written the same way. It takes `127.0.0.1` and not `localhost`, because
resolving a name means `getaddrinfo`, which answers a pointer. Writing it
also showed that `struct sockaddr_in` is different bytes on Linux and
macOS, and that both programs are portable only because macOS forgives
the Linux bytes ([`docs/connect.md`](docs/connect.md)).

There is no socket type and no HTTP library. There **is** now a `Net`
capability: `connect(net, name, port)` dials out and `bind(net, port)`,
`listen` and `accept` take connections in, each checked against the
capability's bound before `getaddrinfo` or `socket` ever runs
([`docs/net.md`](docs/net.md)). `examples/serve/` and `examples/fetch/`,
shown above, predate it and still declare `socket`/`bind`/`listen`/
`accept`/`connect` by hand against `Ffi("libc")` — deliberately not
ported, since `read`, `write` and `close` on the resulting socket still
need `extern fn`, and porting only the calls `Net` now covers would add
a capability to the authority report without removing `Ffi("libc")` from
it. Sockets are libc, libc has a name, and the capability that names it
has existed since M2 — **what decides whether a program is writable
here is not a feature list, it is whether the authority it needs has a
name.**

The same rule says what is out of reach, and it is one sentence: a foreign
*result* is a scalar, so anything that hands back an opaque pointer — TLS,
libpq, `FILE *`, `dlopen` — is not reachable yet, because a pointer from C
carries no region and this language has no reference that lacks one.
Threads are out for a different reason: `pthread_create` wants a function
pointer, and there are no function values; `fork` returns an `int`, so
several processes are fine.

[`docs/reach.md`](docs/reach.md) is the measured version of the gap `Net`
was built to close; [`docs/under-a-grant.md`](docs/under-a-grant.md) is
what a supervisor sees today, since neither program above has been ported
onto the capability that would let its row say `net` instead of
`ffi("libc")`.

And one program here did not start here. `examples/base64/` is GNU
coreutils' `base64`, ported and checked byte-for-byte against it in both
directions. It needed the bit operators, which did not exist, and nothing
else — no new capability, no library, no change to linearity or effect
rows. Its authority report is four lines and every one is checkable from
outside: `args`, `io_read`, `io_write`, `err_write` — the last because it
now says `base64: invalid input` rather than exiting 1 in silence
([`docs/standard-error.md`](docs/standard-error.md)) — and it never
touches the filesystem, the heap or foreign code.
[`docs/porting.md`](docs/porting.md), including §6 on what one small port
does not establish.

`examples/sort/` is the answer to that §6: `LC_ALL=C sort`, five owned
resources on the heap, checked against GNU `sort`. It found four missing
library functions — a vector could be read and appended to but never
written — and left `fs_read`'s inability to report truncation with a
program waiting on it.

---


---

## Related work

lex-sys builds on other people's ideas, and the closest of them got
there first.

- **Austral** — the nearest relative: linear types, capabilities as linear
  values with a root capability handed to the entry point, lexical
  borrowing, no borrow checker. Most of this core is Austral's first.
  lex-sys adds the same authority stated as an **exact effect row**, which
  is what `lex-sys authority` reads.
- **Koka** — effects as a row of labels. Kept the row; no handlers, no row
  polymorphism.
- **Effekt** — *effects as capabilities*, from the effect-handler side:
  the closest statement of "an effect is a borrowed capability".
- **Cyclone** — lexical regions; here without Rust's inference.
- **Rust** — ownership as move; the borrow checker declined.
- **Vale** — generational references, which `Gen` is.
- **Zig** — `defer`.
- **Pony**, **Hylo** — other answers to aliasing and ownership. Pony's
  `val` means something different from this one.
- **The object-capability model** (E, *Robust Composition*) and
  **Capsicum** — no ambient authority.
- **Lex** — the parent language, and the worldview.

**The competitor is WASI, not Rust.** For running code you did not write,
WebAssembly with WASI enforces authority at run time, by trying. lex-sys
knows it **before execution**, from the program's text, with no runtime
cost — a narrower claim, and a stronger one where it applies. Under
`lex-os` it is defence in depth: a static proof before load, a supervisor
while it runs.

What each project contributed, traced to the document that used it, and
what differs: [`docs/related-work.md`](docs/related-work.md).

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


## Performance, honestly

Linearity and effects are erased at compile time; generics monomorphise.

The gap to C is **1.17×–2.58×** across five kernels, and it tracks how
much of the run is in code Cranelift generated
([`benchmarks-game.md`](docs/benchmarks-game.md)). Rust sits within 4% of
C on the same programs, so the gap is **Cranelift against LLVM rather
than the price of ownership**
([`against-c-and-rust.md`](docs/against-c-and-rust.md)).

Two claims this page used to make were false, and both were corrected by
measuring rather than by deleting:

- *"Linearity can hand the optimiser stronger aliasing facts than `&mut`
  does."* It cannot — `&!` is a lock on the *binding*, and `both(s, s)`
  compiles. There is no aliasing fact to emit, and Cranelift has no
  `noalias` to emit it to ([`aliasing.md`](docs/aliasing.md)).
- *The overflow trap costs a never-taken branch.* It costs up to **40.5%**
  in a pure arithmetic loop, because a trap is observable and the loop
  therefore cannot vectorise — and **six of eight** loop-body checks have
  the same property ([`overflow-cost.md`](docs/overflow-cost.md),
  [`check-cost.md`](docs/check-cost.md)).

Removing the trap is necessary and **not sufficient**: without it lex-sys
is still scalar, 2.27× off vectorised clang and 1.55× off *scalar* clang,
so everything remaining is the backend ([`gpu.md`](docs/gpu.md) §2.3).
Cranelift has no aliasing fact to accept, no call-purity attribute to
carry this language's checked purity proof, and no pass that makes
vectors out of scalar code — each a property of its design rather than a
version it has not reached
([`backend-limits.md`](docs/backend-limits.md)).

So **a vectoriser, or a backend with one, is the single largest open
item**, and it is the answer to the number above rather than a
performance nicety.

**It was picked up, and it is no longer hypothetical.** `lex-sys-
codegen-llvm`, behind an additive `--backend cranelift|llvm` flag
(default: `cranelift`), is real and partially built —
[`llvm-backend.md`](docs/llvm-backend.md) tracks each slice. Measured,
not assumed: on `mandelbrot.ls`, `--backend llvm` is statistically
indistinguishable from C (**0.96×–1.00×**), where `--backend cranelift`
still reproduces the **1.6×–1.8×** gap above almost exactly — and
`objdump`, not the wall clock, confirms why on several kernels: once a
loop's trap comes out (`wrapping_add`/`sub`/`mul` in place of `+`/`-`/
`*`), `--backend llvm` actually vectorises it, where the checked twin,
otherwise identical, compiles to zero SIMD instructions. Sixteen of
`benches/`'s programs build through it today, every one this document
tracks: `revcomp.ls` — **42%–46% faster** on `--backend llvm`, once
writing through a reference (`Place::Field`/`Place::Deref`) closed —
`fannkuch.ls` — **22%–28% faster**, once `arg_count`/`arg` closed —
`binarytrees.ls`, once single-value allocation (`alloc`/`box`/`unbox`)
closed too — and `spectral.ls`/`fasta.ls` (**45%–56%**/**10%–20%
faster**), once `float` arithmetic closed. `float`'s own gap turned out
to be structural, not arithmetic: this backend's `LValue` carries no
type tag the way Cranelift's `Value` does, so telling a `float`
operand from an `int` one needed a new helper, `scalar_kind`, reading
the expression that produced a value rather than the value itself.
Matching through a reference — the last gap this document tracked —
closed the same way: the address arithmetic it needed had already been
built for `Place::Field`/`Place::Deref` and structs/enums, so the
whole change was one conditional tag-load plus an address-only mirror
of the existing by-value binding path. **Every gap `llvm-backend.md`
names a target for is now closed.** It is still **not** a complete
backend — `Ffi`/`extern fn` and `Net` are what it refuses, with no
`benches/` program or fixture asking for them yet, deliberately, as an
opt-in and partial backend rather than a finished second one. `Net`
has since been scoped directly rather than waiting on a `benches/`
program: `listen`/`accept`, the two of its four builtins that take no
capability, closed first, then `bind` itself — `socket`+
`setsockopt(SO_REUSEADDR)`+`bind` folded into one call, the same
`struct sockaddr_in` `lex-sys-codegen`'s own `bind` builds by hand,
and the first `--backend llvm` expression needing a value conditional
on which of three runtime paths ran rather than trapping or writing
into an already-`alloca`'d `var`. A differential test now builds a
listener on each backend, connects a real `TcpStream`, and checks
both accept it — the first Net-capable program `--backend llvm` has
ever actually run. `connect` and `Ffi`/`extern fn`, the only way to
reach the network any other way, are still refused.
[`ROADMAP.md`](docs/ROADMAP.md) says what's next.

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
lex-sys build <file.ls>... [-o <output>] [--emit exe|obj] [--std] [--backend cranelift|llvm]
lex-sys check <file.ls>... [--std] [--output json] [--backend cranelift|llvm]   # refuse, or say nothing
lex-sys run   <file.ls>... [--std] [--backend cranelift|llvm]   # build, run, exit with the program's status
lex-sys ids   <file.ls>... [--std]    # each declaration's content hash
lex-sys authority <file.ls>... [--std] [--output json]  # what it can reach
lex-sys layout    <file.ls>... [--std]  # what every leaf costs, and what packing would save
lex-sys print <file.ls>               # the unit, rendered in canonical form
lex-sys agent-guidelines              # AGENTS.md, from inside the binary
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
report as data, and it **fails closed**: its first field is `"bounded"`,
`false` for any program that reaches foreign code, because a library is
not an authority domain and `ffi("libc")` bounds nothing.

That was learned by trying. [`docs/under-a-grant.md`](docs/under-a-grant.md)
checked the report against `lex-os`'s real grant: the filesystem
dimension is enforceable, and **more precisely than the grant can
express**; network and exec are not, because sockets and processes are
libc. So a supervisor that reads nothing but `bounded` refuses exactly the
programs it cannot see into.
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
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | Working on the compiler: the gate, how a slice is done, the file budget, and code conventions |

Design lands in `docs/` **before** the code that implements it, which is the
cheap place for it to be wrong. When it turns out wrong anyway, the document
that made the claim is corrected in place rather than quietly edited — four
of them carry a correction now, and the roadmap says which.

## Licence

[EUPL-1.2](LICENSE), matching the rest of the ecosystem. See `LICENSE` for
the notice and where to obtain the full text in any of the 23 EU languages.
