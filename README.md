# lex-sys

A **systems dialect carrying Lex's philosophy**: native compilation, no GC, linear
ownership and capability-typed effects unified into one resource system, fully
defined behaviour, and a canonical content-addressable AST designed in from day one.

> **Status: M1 complete, M2 complete, M3 complete.** A bootstrap compiler takes a
> `.ls` file to a real native executable, and CI proves it on **linux-x86_64
> and darwin-aarch64**. The language has a type system: `int` and `bool`,
> structs, enums with exhaustive pattern matching, and monomorphised generics.
> Every declaration has a content hash (`lex-sys ids`), and `lex-sys print`
renders a unit back in canonical form — the other direction of the same
pipeline, checked by a test that round-trips every file in the repository
and requires every hash to survive.
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
> **And there is allocation.** `region a { .. }` opens an arena, `alloc[a](v)`
> puts a value in it, and the whole thing is released in one call at block
> exit — no traversal, no finalisers. It is the same region machinery as a
> borrow, checked by the same occurs-check, which is what §5's lexical bet
> was for. **M2 is complete.**
>
> **And arithmetic is checked.** `int` is 64-bit two's complement, and `+`,
> `-`, `*` and unary `-` produce the right answer or **trap** — they never
> wrap. Wrapping is still expressible, but it has to be asked for by name
> (`wrapping_add`), because a silently wrong answer is worse than a stopped
> process. Evaluation order is left to right everywhere, including a struct
> literal's fields, which is enforced rather than merely intended. That is
> [`docs/defined-behaviour.md`](docs/defined-behaviour.md), and every rule in
> it has a fixture.
>
> **And there are slices.** `&!a [T]` is a run of values in an arena, with
> its length beside the pointer. It is an *ordinary reference* — same
> regions, same escape check, same unique-to-shared coercion — so slices
> needed no second mechanism. Every index is bounds-checked and an
> out-of-range one traps, with no unchecked form to reach for.
>
> **And there are strings.** A string is a run of bytes and claims no
> encoding: `&r [byte]`, an ordinary slice, so nothing about regions,
> escape or coercion had to be written twice. `byte` is storage rather than
> arithmetic — `byte_of` / `int_of` convert, and `byte_of` traps rather than
> truncating — and a byte slice crosses to C as a pointer and a separate
> length, so `write(fd, ptr, len)` is expressible. That is
> [`docs/strings.md`](docs/strings.md), settled before any code existed.
>
> **And it reads and writes files.** `Fs(prefix)` is a capability carrying a
> path, narrowed the way every other capability is — except that a path
> prefix extends at a `/`, so `/tmp` contains `/tmp/a` and pointedly does
> not contain `/tmpevil`. The effect labels carry the prefix, so a row says
> *which part of the filesystem* a function touches. The operations are
> builtins rather than `extern fn` on purpose: an `extern` would be gated by
> `Ffi("libc")`, and then `Fs` would be decoration. A path outside the
> granted prefix traps, a path containing `..` traps rather than being
> normalised, and a missing file is `-1`. That is
> [`docs/filesystem.md`](docs/filesystem.md), which is also honest about
> what `Fs` is not: not a sandbox, because `Ffi("libc")` plus an `extern`
> reaches any path. What it is, is authority visible in the types.
>
> **And there is a heap.** `Heap` is the fifth capability and `Box[T]` is
> one value in one allocation. It is `res` whatever `T` is, so §4's
> exactly-once rule applies unchanged and **the general heap cannot leak** —
> a box that is never unboxed is a *compile error*, and a double free is
> unexpressible because `unbox` consumes. It costs nothing at run time: a
> box is a pointer, with no header, no refcount and no tag. The payoff is a
> type that can contain itself, through a `Box` and only through one — so
> linked lists and trees compile, and the walk that reads one is the walk
> that frees it. That is [`docs/heap.md`](docs/heap.md).
>
> **And a reference can be read.** `*r` reads what a reference points at,
> and `match` on a reference binds each payload as a reference into the
> value — one rule, *a reference gives references*, with no binding modes
> to infer. So a recursive structure can be traversed without being
> destroyed, which it could not be a day ago, and a function can hand a
> result back through a `&!r` out-parameter. That is
> [`docs/reading-references.md`](docs/reading-references.md).
>
> **And it reads its command line.** `Args` is the sixth capability, and
> the interesting part is *why* it is one: arguments grant no power, so
> this is not about containment. It is about visibility — a function whose
> behaviour depends on the command line should say so in its type, and
> ambient argv would let one eight frames down branch on `--force` with
> nothing in any signature admitting it. `examples/lines.ls` is a real CLI
> tool now. That is [`docs/arguments.md`](docs/arguments.md).
>
> **And a program can be more than one file.** Named on the command line,
> sharing one flat namespace — no `import`, no namespaces, no visibility,
> which is deliberately the minimum that makes a *library* possible at
> all. `examples/wordfreq/` is the capstone: a byte-helper file, a tally
> file and a program, using every capability the language has. It also
> makes `canonical-ast.md` §1 checkable for the first time — the same
> function in two different files has the same hashes, which has been the
> claim since M0 and until now had no files to be tested across. That is
> [`docs/many-files.md`](docs/many-files.md).
>
> **And a run of values can live on the heap.** `Box[[T]]` is the second
> shape a box comes in — a pointer *and* a length — and it is what every
> collection needs: an arena slice is bounded by its block and an ordinary
> box holds one sized value, so until now a buffer that grows could not
> exist. The language has no `grow`, `push` or `realloc`; growing is
> allocate-copy-end, and `examples/buffer/` writes it down in sixty lines
> so the doubling policy belongs to the program. That is
> [`docs/boxed-slices.md`](docs/boxed-slices.md).
>
> **And sharing has an answer, which is not the one the design expected.**
> `linearity-and-effects.md` §9 promised two escape hatches for the
> structures linearity cannot express, `Rc` and `Gen`, "both libraries,
> not language features". Building them found that is true of `Gen` and
> **false of `Rc`**: an `Rc` needs a copyable pointer, and this language
> has none — three different ways of writing one are three fixtures in
> `tests/reject/`, each refused by a different rule that was not written
> with `Rc` in mind. `Gen` works precisely because a handle *points at
> nothing*, and `examples/slab/` is it. That is
> [`docs/sharing.md`](docs/sharing.md), which corrects the document it
> implements.
>
> **And a function can answer with two things.** `(A, B)` is an anonymous
> struct with positional components, and the first **structural** type
> here: `(int, Gen)` is the same type wherever it is written, so two
> files agree on one with neither declaring it. Its mode is *computed* —
> `res` if any component is — because there is no declaration site to
> write one at, and every obligation that follows from `res` follows
> anyway. It is measurably nothing but ergonomics: a tuple and the struct
> it replaces emit **byte-identical object files**, which is a test
> rather than a claim. That is [`docs/tuples.md`](docs/tuples.md), and it
> is the first feature here whose case was made by a library rather than
> by a design — `examples/slab/` asked for it, and has since lost two
> types and a whole function.
>
> **And a name can be rebound.** Shadowing within a block was refused
> outright, and that was not arbitrary: `let held = ...` twice would put
> the first allocation out of reach with its obligation undischarged,
> which is the leak affine types drop silently. But the refusal was
> broader than the hazard. The rule now is **a binding may be shadowed
> exactly when it is dead** — which is the rule assignment already had,
> so it is one rule with two syntaxes rather than a new one. It is
> checked at replay rather than in the parser, because whether a binding
> is dead is a fact about the trace and not about the source. That is
> [`docs/shadowing.md`](docs/shadowing.md), and it finishes
> `sharing.md` §4's list.
>
> **And it reads standard input.** `getchar` is the mirror of `putchar`:
> one byte in, behind the capability that authorises it, `-1` at the end.
> What is worth reading is that it is **not a seventh capability**. `Fs`
> is one capability with two labels, `fs_read` and `fs_write`, because
> reading a file and writing one are the same authority used in two
> directions and the *row* says which. The console is the same shape — so
> `Io` gains `io_read`, and `putchar`'s label becomes `io_write`. A
> function declaring `[io_write]` may not read, and the refusal names the
> label it is missing; without that the two labels would be a spelling
> rather than a distinction. `examples/tally.ls` is `wc` over a pipe,
> which is the program that could not be written before. That is
> [`docs/standard-input.md`](docs/standard-input.md).
>
> **And a name can live in a namespace.** `module a.b;`, `import a.b;`
> (or `as`), `pub`, and qualified names — the precondition for a
> *standard* library, which one flat namespace could not have. The claim
> worth reading is that **a module reaches no hash**: `canonical-ast.md`
> §1 has said since M0 that moving a function between files changes
> nothing about it, and a module could have broken that. It does not,
> because a call already encodes the callee's *hash* rather than its
> spelling — so moving a function into a module changes neither its own
> hashes nor any caller's, and there is a test that says so. A module is
> also **not a trust boundary**: `pub` means reachable, never safe, and
> a public function still needs a caller holding the capability and
> still declares what it did. That is
> [`docs/modules.md`](docs/modules.md).
>
> **And there is a standard library.** `std.bytes` (text, which here
> means bytes), `std.io` (the console), `std.math`, and `std.buffer` —
> the one collection, because every other one wants generics over a
> *mode* and there are none yet. Reached with `--std`, whose source is
> compiled into the `lex-sys` binary rather than looked up, so it adds
> no search path and no manifest. It is opt-in and never a prelude: a
> program still writes `import std.io;` where it uses one.
>
> Building it found the compiler emitting **every** non-generic function
> rather than what `main` reaches — 6720 bytes against 1048 for a
> program calling none of the library. Fixed rather than documented
> around: checking is total, emission starts at the entry point, and
> every program gets the benefit. That is
> [`docs/standard-library.md`](docs/standard-library.md).
>
> **And a type parameter can say which modes it works at.** Checking
> §12's claim that mode polymorphism was "half-answered" turned up a
> **leak and a double free** underneath it: `val struct Wrap[T]` was
> *trusted* rather than checked, so `Wrap[Box[int]]` was `val` by
> assertion — discardable, and copyable. Both compiled; valgrind said
> *8 bytes definitely lost* and *Invalid free()*.
>
> The fix and the missing feature are one mechanism. Declaring an
> aggregate `val` is a bound on its own parameters, and now functions
> can write one: `[T: val]` is checked once and enforced at the **call
> site**, while an unbounded `[T]` is checked as though it were `res` —
> the stronger obligation, so a body that passes is safe at every
> instantiation and its errors land on the definition. There is no
> `[T: res]`, because it would mean what unbounded already means. That
> is [`docs/mode-polymorphism.md`](docs/mode-polymorphism.md).
>
> It also found a second, worse bug: a call resolved its **effect row**
> and its **types** through two different lookups, so a function calling
> into C could declare `[]` and compile. Fixed — one resolution.
>
> **Still missing:** a writer abstraction, and `Option`/`Result` over
> resources — neither blocked by the language any more, both wanting a
> library design. Do not mistake this for a usable language yet.

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
   Written out operation by operation in
   [`docs/defined-behaviour.md`](docs/defined-behaviour.md).
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
# M2 arena: 1 4 9 -> 14
# M3 arithmetic: 6 1
# M3 slice: 3 1 4 1 5 -> 14
# M3 string: hello (5 bytes, e is 101)
# M3 file: on disk (7)
# M3 heap: 3 1 4 -> 8 (freed)
# M3 reading: 8 3 8 (kept)
# M3 args: 1 (named)
```

`examples/tour.ls` is the shortest honest answer to "what can this language
do": one section per feature, in the order the milestones added them.

Programs rather than a tour:

```sh
cargo run -p lex-sys -- run examples/pipeline.ls
# jobs: 4 done, 2 cancelled
# spent: 49 of 50
# headroom: 1
```

`examples/pipeline.ls` admits a run of jobs against a budget, and is M2 as
*one* system rather than four features side by side. A job is a linear
resource finished exactly once — completed or cancelled, never both and
never neither; deciding which needs its cost, and reading a field of
something you own without spending it is a borrow; the running tally lives
in an arena; the overrun is computed by libc through a capability that names
libc and nothing else; and printing needs the console capability `main` was
handed. Delete any one of those and it stops compiling.

`examples/tree.ls` is a binary search tree — the shortest honest answer to
"why does a language need a heap at all". Its shape is decided at run time,
its nodes outlive the calls that made them, and the type is defined in terms
of itself; none of that fits in a block. There is **no `free` in the file**,
and every node is freed: `unbox` is the only thing that ends a box, a box is
`res`, and a node the program forgot would be a compile error rather than a
leak found next month.

```sh
cargo run -p lex-sys -- run examples/tree.ls
# 1 3 4 5 7 8 9
# sum 37 count 7 depth 3
```

`examples/lines.ls` is the M3 acceptance criterion: a real command-line
tool. Given a path it reads that file, counts and filters it, writes a
report and reads the report back; given nothing it lays down a sample and
reads that, which is how a tool with no input behaves anyway.

```sh
cargo run -p lex-sys -- run examples/lines.ls
# lines 6
# errors 2
# longest 15

lines /var/log/app.log        # or a path you name
```

It is also where the design shows a sharp edge honestly. A capability is
narrowed to a **literal**, checked where it is written — so a tool that
reads a path the *user* chose cannot narrow to it, because there is no
literal to narrow to. `lines` therefore passes its `Fs` on unnarrowed, and
narrowing it to `/tmp` would not make a safer tool, it would make a broken
one. What the types still buy at that width: `report`'s row says
`fs_read("")` and `fs_write("")`, so a reader knows it reads and writes
arbitrary paths without opening the body. A tool that *did* know its
directory would narrow, and its row would say so instead. The type tells
the truth either way.

`examples/buffer/` is a growable byte buffer, written as a library. It is
worth reading for `reserve`, which is the *entire* implementation of
"growing": a bigger box, a copy, and the old one ended. The language has
no `realloc` — which would also hide which of two very different things
happened — so the doubling policy is in this repository rather than in the
compiler, and the cost is countable: building 31 bytes from a one-byte
buffer is three allocations, which is what valgrind reports.

```sh
cargo run -p lex-sys -- run examples/buffer/main.ls examples/buffer/buffer.ls
# counting: 1 4 9 16 25 36 49 64
```

`examples/slab/` is shared ownership, as far as this language reaches: a
slab that owns every value and hands out `Gen { index, generation }`
handles, which are two plain `int`s and copy like any other `val`. The
line worth watching is the last one — a handle whose slot was removed
comes back `Missing`, which is a **value the program decides what to do
about** rather than a dangling pointer it gets no say in.

It is also the before-and-after for tuples and shadowing. Writing it is
what found both gaps. `insert` and `look` each had to answer with a slab
*and* something else, so each declared a `res struct` that was not a
concept in the library; one function had to be split in two because a
struct pattern cannot rename what it binds; and `run` had eight names
for one slab because a name could not be rebound. All three are gone,
no rule was weakened, and the object file did not change.

```sh
cargo run -p lex-sys -- run examples/slab/main.ls examples/slab/slab.ls
# live handle:  7
# after remove: missing
# new handle:   9
# old handle:   missing
```

`examples/wordcount.ls` is the before-and-after for the standard
library. It used to open with `space`, `newline`, `write_all`,
`print_nat` and `is_blank` — five helpers that are not what the program
is about, and that 24 other files here each wrote out again. They are
`std.io` and `std.bytes` now:

```sh
cargo run -p lex-sys -- run examples/wordcount.ls --std
```

`is_blank` moving out is the part worth noticing. `wordcount` counted
space and newline; `tally` counted six bytes; neither knew the other
disagreed. One definition, in one place, is most of what a standard
library is for.

`examples/modular/` is two modules and a root. `fmt.text` holds the
console helpers, `fmt.counts` holds a tally and imports `fmt.text`, and
`main` imports both — one under its own name and one renamed with `as`,
because a qualifier is a name the importing file chose rather than a
path. It is also where `pub` earns its place: `counts.step` is private,
so it is an implementation detail rather than a promise.

```sh
cargo run -p lex-sys -- run examples/modular/main.ls \
    examples/modular/counts.ls examples/modular/text.ls
# seen 3, total 60
# 60
```

`examples/wordfreq/` is the capstone, and the only example that is three
files: `text.ls` holds byte helpers, `counts.ls` holds the tally, and
`main.ls` is the program. Every capability is in it doing real work —
arguments, file IO, the heap, matching through references, slices — and
`bump` is worth reading in particular: it walks the tally through a
*unique* reference and increments a count in place, which is what matching
`&!c` is for.

```sh
cargo run -p lex-sys -- run examples/wordfreq/main.ls \
    examples/wordfreq/text.ls examples/wordfreq/counts.ls
# dog 1
# lazy 1
# over 1
# jumps 1
# fox 2
# brown 1
# quick 1
# the 3
```

`examples/tally.ls` is `wc` over **standard input** — the same counts,
from a pipe rather than from a document compiled in. Worth reading for
`count`'s row: `[io_read, io_write]` says in the signature that the
function consumes the program's input as well as writing to the console,
and a caller learns both without opening the body.

```sh
printf 'the quick brown fox\njumps over\nthe lazy dog\n' \
    | cargo run -p lex-sys -- run examples/tally.ls
#      3      9     44
```

`examples/wordcount.ls` is `wc` over an embedded document — the first
program here that is mostly text processing rather than demonstration, with
a whole-word search that is two slices compared a byte at a time, which is
all a string comparison is when a string is bytes.

`examples/rational.ls` is a real 250-line program — exact rational arithmetic
with a generic `Result[T]` threaded through every fallible operation. It
predates M2 and stays that way on purpose: it is the M1 language still
compiling unchanged.

**What exists:** `int` and `bool`, functions and calls, arithmetic and
comparison, `&&`/`||` with short-circuiting, `if`/`else`, `while`, `let`/`var`
bindings, structs, enums with exhaustive `match`, generics over both,
`res`/`val` modes with exactly-once linearity and destructuring `let`,
shared and unique borrows with lexical regions, exact effect rows,
capabilities, narrowing, capability-gated foreign calls, arenas, checked
arithmetic, slices, strings, file IO through a path-carrying capability, a
general heap with recursive types, boxed slices and the growable buffers
they allow, reading through references — `*r` and `match` on a reference —
tuples, the command line, standard input, programs spread over several
files, modules with visibility, and a standard library.

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

Data that has to outlive the block that made it goes in an arena:

```
region a {
    let node = alloc[a](Node { value: 1 });   // node : &!a Node
    total = total + value_of(node);
}                                             // released here, in one call
```

An arena *is* a region — the same block, the same parent chain, the same
occurs-check — which is what the lexical bet in §5 was for. Nothing whose
type mentions `a` leaves the block, an inner arena may hold what an outer
one allocated and never the reverse, and release is one `free` whatever was
allocated. Arenas hold `val` data only: releasing one reclaims memory and
runs nothing, so a linear value inside would have its obligation dropped
rather than discharged. Asking for more than the chunk holds traps, because
the alternative is writing past an allocation and this language has no
undefined behaviour to do that in.

A string is a run of bytes, and claims no encoding:

```
fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io] int { ... }

write_all(i, "Hello, world!\n");     // &static [byte], shared, in the object file
```

`str` is not a type. A string is `&r [byte]` — an ordinary slice, therefore
an ordinary reference — so regions, the escape check, the unique-to-shared
coercion and `val` mode all came for free. A validated string type would
have to answer what an *invalid* one is, and every answer costs a fallible
constructor everywhere or a lie somewhere; decoding is library work over
this slice type. `byte` is **storage, not arithmetic**: no `+`, with
`byte_of` and `int_of` converting and `byte_of` trapping rather than
truncating. A byte slice crosses to C as a pointer plus a separate length,
so `write(fd, ptr, len)` is expressible and `strlen(ptr)` is not.

A run of values is a slice, which is a reference like any other:

```
region a {
    let xs = alloc_slice[a](5, 0);      // xs : &!a [int]
    xs[0] = 3;                          // bounds-checked; out of range traps
    total = sum_of(xs);                 // &!a [int] where &r [int] is wanted
}
```

`[T]` is the referent — a run of `T`s whose length is a runtime value — and
`&!a [T]` points at one. Because it is an ordinary reference it carries a
region, cannot escape it, coerces from unique to shared, and is `val`; none
of that needed a second mechanism. The length travels beside the pointer,
which is why `len` reads a value rather than computing one and why every
index can be checked against it. There is no unchecked indexing and no
release mode that removes the check.

There is **no borrow checker**. A region is a block, so a reference's validity
is lexical rather than inferred: no non-lexical lifetimes, no variance, no
dataflow. A binding is `Owned`, `Frozen` or `Locked`, set at block entry and
restored at block exit; `r_inner <= r_outer` holds exactly when the outer
block encloses the inner one, which is a walk up a stack; and escape is an
occurs-check over one type.

File IO closed the last mile to M3's acceptance criterion, and it needed no
new mechanism either: `Fs(prefix)` is the capability §8.1 of the M2 document
listed as waiting, narrowed by the same prefix extension `Ffi` uses, and the
operations are two builtins that take it. What was genuinely new was two
decisions rather than any machinery — that the operations must *not* be
`extern fn`, or `Ffi("libc")` would subsume them, and that a path outside
the granted prefix traps while a path containing `..` is refused rather than
normalised. See [`docs/filesystem.md`](docs/filesystem.md).

The heap closed M2's last unchecked item, and it is where the "one system"
claim paid off most visibly. `Box[T]` is `res`, so §4's exactly-once rule —
written for capabilities and file handles — turns out to be a *memory
safety* rule for free: no leaks, no double frees, no use-after-free, none of
them checked by anything new. `contents` is region-preserving, so §5's
occurs-check refuses a reference escaping a box exactly as it refuses one
escaping an arena, by the same code. The only thing genuinely added to the
checker was one hole in the size check: a type may contain itself through a
`Box`, because a box is one pointer however large what it points at is.

Both limits that slice found are now closed, by the change that came next.
They looked like two problems — a scalar behind a reference could not be
read, and a recursive structure could not be read without destroying it —
and they were one: nothing could be read *through* a reference except a
field or an element. One rule fixes both. **A reference gives references:**
matching `&l List` binds every payload as a reference into the list,
carrying the scrutinee's mode and region, and `*r` reads a `val` referent.
Nothing moves out of a reference, so a `res` payload binds as a borrow and
linearity is untouched; there are no binding modes to infer, because the
scrutinee decides and it is one line up.

Command-line arguments answered the last capability question M3 had open,
and the answer is worth reading even though the feature is two operations.
Arguments grant no *power*: a program that reads argv cannot damage
anything by doing so, and `Fs` still governs what it may open. So the case
for making them ambient was real. It loses because an effect row here is
about **visibility** rather than containment — a function whose behaviour
depends on the command line should say so in its type, and ambient argv
would let one eight frames down branch on a flag with nothing in any
signature admitting it. `putchar` is not more dangerous than `arg`; it is
more visible, and that is what an exact row is for.

More than one file came last, and it is the smallest feature here with the
largest consequence: three design docs had each had to write *"there is
nowhere to put a library"*, and now there is. A program is the set of files
named on the command line, sharing one flat namespace — no `import`, no
namespaces, no visibility, because each of those is a design and the
minimum that unblocks a library is none of them.

It is also where `canonical-ast.md` §1 stopped being aspirational. That
section has said since M0 that "moving a function between files changes
nothing about it"; with one file there was nothing to test. The same
function in two files now demonstrably has the same `SigId` and `BodyId`.

Sharing came after that, and it is the one slice that changed a design
document rather than implementing it. §9 promised `Rc` and `Gen` as two
libraries; `Gen` is one, and is `examples/slab/`. `Rc` is not, and cannot
be — it needs a value that copies *and* names an allocation, and every
value here is one or the other. Three ways of writing it are three
fixtures in `tests/reject/`, refused by three unrelated rules. A copyable
pointer is not a feature this language is missing; it is one the language
is made of not having.

Tuples came last, and they are the only feature here that a *library*
asked for rather than a design. `sharing.md` §4 listed three things that
made `examples/slab/` more verbose than it should have been; `(A, B)`
closed two of them, and the slab lost two declared types and one whole
function. The interesting part is what it cost, which is nothing: a
tuple and the struct it replaces compile to byte-identical object files,
and there is a test that says so. A tuple is also the first structural
type here — no declaration, so its identity is its components and two
files can agree on one with neither declaring anything.

Shadowing came after that and finished the list. It is the one slice
where the restriction turned out to be a real rule stated too bluntly:
rebinding a name that still holds a `res` value strands that value, and
that is a leak. So the rule is now the one assignment already had — a
binding may be shadowed exactly when it is dead — and the refusal
survives for the program that was actually dangerous. It is checked at
replay rather than in the parser, because whether a binding is dead is a
fact about the trace. One must-reject fixture was retired to it.

Standard input came last, and the interesting part was where it went.
The obvious move is a seventh capability and an eighth field on `Split`;
the right one is a second *label* on `Io`, because `Fs` already answers
this question — one capability, two directions, one label each, and the
row says which. So `putchar`'s effect is `io_write` now and reading is
`io_read`, a rename across 53 files that had to happen with the feature
rather than after it. `examples/tally.ls` is `wc` over a pipe.

Modules came last, and they are the precondition for a *standard*
library rather than merely a library. The evidence for needing them is
countable: across this repository's examples and fixtures, `print_nat`
was defined 25 times byte for byte and `write_all` 19 times, and putting
them in one flat namespace would have meant a standard library owning
names programs here already use.

The claim worth checking is that **a module reaches no hash**. A call
has encoded the callee's *hash* rather than its spelling since M0, for
an unrelated reason, so moving a function into a module changes neither
its own identity nor any caller's — `canonical-ast.md` §1 survives
namespaces, and there is a test that compiles the same two functions
flat and modular and asserts all four hashes are identical.

The standard library came last, and building it found a real cost the
compiler had been paying all along. A program that called none of it
still got 6720 bytes of object against 1048 without — because emission
seeded from *every* non-generic function rather than from what `main`
reaches, which is indistinguishable from reachability right up until
there is a library. The two passes have one job each now: checking is
total, emission starts at the entry point. Every program gets it, not
just ones using `std`.

It also surfaced a must-reject fixture that had been **passing for the
wrong reason** — `effect_not_propagated.ls` was refused for a missing
argument rather than for the effect rule it tests, and only the change
in checking order revealed it.

Mode polymorphism came last, and it is the slice that found the most.
§12 called it "half-answered" — monomorphisation already makes a generic
work at both modes — and checking that turned up a **leak and a double
free** hiding under the half that was answered: a `val` on a generic
declaration was trusted rather than checked, so `Wrap[Box[int]]` was
`val` by assertion, discardable and copyable.

The fix is the feature. Declaring an aggregate `val` bounds its own
parameters, and functions can now write that bound: `[T: val]` is
checked once and enforced at the call site, while unbounded means
checked as `res` — so a generic that drops its parameter is refused
where it is written rather than wherever somebody first used a resource
type. Four functions in this repository needed the bound, and each
genuinely only ever worked for copyable types.

It also found a worse bug, and one that modules had introduced: a call
resolved its **effect row** and its **types** through two separate
lookups with different precedence, neither module-scoped. A function
calling into C could take its types from the `extern` and its effects
from an unrelated same-named function elsewhere — declaring `[]`, and
compiling. The effect system is the whole point of the language, so that
is the most serious kind of bug it can have: not a crash, a lie.

**What does not, yet:** a writer abstraction, `Option` and `Result` over
resource types, flag parsing, environment variables, and standard error.
None of them is blocked by the language any more — the first two want a
library design, the rest are ordinary work.

Every example declares what it prints in its own header, and a test walks
`examples/` and checks them, so an example that stops matching the language
fails CI rather than quietly rotting.

The compiler's whole surface:

```sh
lex-sys build <file.ls> [-o <output>] [--emit exe|obj]
lex-sys check <file.ls>     # refuse or say nothing
lex-sys run   <file.ls>     # build, run, exit with the program's status
lex-sys ids   <file.ls>     # each declaration's content hash
lex-sys print <file.ls>     # the unit, rendered in canonical form
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
| [`docs/linearity-and-effects.md`](docs/linearity-and-effects.md) | The M2 gate: linear ownership, capability-typed effects, how they unify, and 23 must-reject fixtures written out as the conformance suite | settled; §3 through §8 implemented |
| [`docs/bootstrap.md`](docs/bootstrap.md) | What M0 settled: bootstrap host (Rust), extension (`.ls`), the M0 surface, what is scaffolding and what replaces it | written |
| [`docs/canonical-ast.md`](docs/canonical-ast.md) | Canonicalisation rules and per-unit identity: what is hashed, and what a hash is allowed to change with | written, implemented |
| [`docs/boxed-slices.md`](docs/boxed-slices.md) | `Box[[T]]`: a pointer *and* a length, and the three operations a run of heap values needs | **settled and built** — the foundation every collection wants; `examples/buffer/` is the growable buffer |
| [`docs/many-files.md`](docs/many-files.md) | A program in more than one file: flat namespace, identity by content, global spans | **settled and built** — the precondition for a library of any kind |
| [`docs/arguments.md`](docs/arguments.md) | The `Args` capability, `arg_count` / `arg`, and why reading argv is an effect | **settled and built** — the "command-line" half of M3's acceptance criterion; §7's must-reject suite is enforced |
| [`docs/reading-references.md`](docs/reading-references.md) | `*r`, and `match` on a reference binding payloads as references | **settled and built** — closed the two limits the heap slice left open; §7's must-reject suite is enforced |
| [`docs/heap.md`](docs/heap.md) | The `Heap` capability and `Box[T]`: why the heap cannot leak, recursive types, heap versus arena | **settled and built** — closes M2's last unchecked item; §8's must-reject suite is enforced |
| [`docs/filesystem.md`](docs/filesystem.md) | The `Fs(prefix)` capability, why the operations are builtins rather than `extern fn`, the runtime path check and why `..` is refused | **settled and built** — the last mile to M3's acceptance criterion; §7's must-reject suite is enforced |
| [`docs/sharing.md`](docs/sharing.md) | §9's escape hatches as built: why `Rc` needs a copyable pointer this language does not have, why `Gen` does not, and what a linear library costs to write | **settled and built, and it corrects `linearity-and-effects.md` §9** — three reject fixtures for the three ways `Rc` fails; `examples/slab/` is the one that works |
| [`docs/tuples.md`](docs/tuples.md) | `(A, B)`: an anonymous struct with positional components, the first structural type here, and a computed rather than declared mode | **settled and built** — the first feature whose case was made by a library; a tuple and the struct it replaces emit byte-identical objects |
| [`docs/shadowing.md`](docs/shadowing.md) | Rebinding a name in one block: why it was refused, and the liveness rule that replaces the refusal | **settled and built** — the last of `sharing.md` §4's three gaps, and one rule where there were two |
| [`docs/standard-input.md`](docs/standard-input.md) | `getchar`, and why reading the console is a second label on `Io` rather than a seventh capability | **settled and built** — `examples/tally.ls` is `wc` over a pipe; the `io` effect label became `io_write` |
| [`docs/modules.md`](docs/modules.md) | `module`, `import`, `pub` and qualified names — a namespace, not an identity, and not a trust boundary | **settled and built** — the precondition for a standard library; a module reaches no hash, and a test says so |
| [`docs/standard-library.md`](docs/standard-library.md) | `std.bytes`, `std.io`, `std.math`, `std.buffer`, and `--std` with the source in the binary | **settled and built** — and it found the compiler emitting unreachable code; a program with `--std` and one without now emit byte-identical objects |
| [`docs/mode-polymorphism.md`](docs/mode-polymorphism.md) | `[T: val]`, and what unbounded means: checked as `res`, so the error lands on the definition | **settled and built** — and it found a leak, a double free, and an effect row that could come from the wrong function |
| `docs/memory-model.md` | Regions, escape, the escape hatches and their cost | not written — §5 and §6 settled and built regions and escape, and `sharing.md` has now settled §9's hatches. Nothing is left that a document of its own would say |
| [`docs/defined-behaviour.md`](docs/defined-behaviour.md) | Every place C and Rust leave behaviour open, and what we define it to | **written and enforced** — overflow traps, evaluation order is left to right, and §9 names the fixture behind each rule |
| [`docs/strings.md`](docs/strings.md) | What a string is: bytes rather than an encoding, `byte` as storage rather than arithmetic, packed layout, literals and the static region | **settled and built** — gated M3's last item; §9's must-reject suite is enforced |

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
| **M2** — the actual thesis ([#2](https://github.com/alpibrusl/lex-sys/issues/2)) | Linear ownership, effect rows and capability-passing as **one** system | **complete** — §3 through §8 of the design document, every must-reject fixture enforced. §9's last item, a heap whose cost is documented, is [`docs/heap.md`](docs/heap.md); its two *sharing* hatches are [`docs/sharing.md`](docs/sharing.md), which found that only one of the two is a library |
| **M3** — minimal but real | Slices and strings, arenas, libc FFI, settled overflow semantics, canonical printer, per-unit identity, file IO through `Fs` | **complete.** `examples/lines.ls` is the acceptance criterion: a tool that reads and writes files, counts and filters, and whose authority to do any of it is one narrowed capability |

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
