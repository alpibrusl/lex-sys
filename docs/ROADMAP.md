# Roadmap

Where `lex-sys` is, what got it here, and what is next. The README says
what the language *is*; this file is the only place that tracks its
history, so the README does not have to grow a paragraph per change.

Tracked in the epic: **[#1](https://github.com/alpibrusl/lex-sys/issues/1)**
— milestones with acceptance criteria, sequencing, risks and open
decisions.

---

## Milestones

| Milestone | What | Status |
|---|---|---|
| **M0** — native hello world ([#3](https://github.com/alpibrusl/lex-sys/issues/3)) | Lexer, parser, AST, IR, Cranelift backend, a real executable | **done** — green on linux-x86_64 and darwin-aarch64 |
| **M1** — typed core | Type checker, `bool`, structs, ADTs with exhaustive `match`, monomorphised generics. No linearity, no effects — deliberately | **done** |
| **M2** — the actual thesis ([#2](https://github.com/alpibrusl/lex-sys/issues/2)) | Linear ownership, effect rows and capability-passing as **one** system | **done** — §3 through §8 of [`linearity-and-effects.md`](linearity-and-effects.md), every must-reject fixture enforced |
| **M3** — minimal but real | Slices and strings, arenas, libc FFI, settled overflow semantics, canonical printer, per-unit identity, file IO through `Fs` | **done** — `examples/lines.ls` is the acceptance criterion: a tool that reads and writes files, counts and filters, and whose authority to do any of it is one narrowed capability |

Everything below M3 is post-milestone work, shipped one slice at a time.

---

## What has landed

One row per slice, newest last. Each links to the document that holds the
reasoning — this table is the index, not the argument.

| # | Slice | The claim worth remembering |
|---|---|---|
| [#22](https://github.com/alpibrusl/lex-sys/pull/22) | [A general heap](heap.md) | `Box[T]` is `res`, so §4's exactly-once rule turns out to be a **memory safety** rule for free: no leaks, no double frees, no use-after-free, none of them checked by anything new |
| [#23](https://github.com/alpibrusl/lex-sys/pull/23) | [Reading through a reference](reading-references.md) | **A reference gives references.** One rule closed two limits that looked unrelated, and there are no binding modes to infer because the scrutinee decides |
| [#24](https://github.com/alpibrusl/lex-sys/pull/24) | [Command-line arguments](arguments.md) | An effect row is about **visibility**, not containment. Arguments grant no power, and they are still an effect, because a function that branches on `--force` should say so in its type |
| [#25](https://github.com/alpibrusl/lex-sys/pull/25) | [A program in more than one file](many-files.md) | The smallest feature with the largest consequence: three design docs had each had to write *"there is nowhere to put a library"*. It also made `canonical-ast.md` §1 testable for the first time |
| [#26](https://github.com/alpibrusl/lex-sys/pull/26) | [Boxed slices](boxed-slices.md) | `Box[[T]]` is a pointer *and* a length — what every collection wants. Found a double free reachable from ordinary code |
| [#27](https://github.com/alpibrusl/lex-sys/pull/27) | [Sharing](sharing.md) | `Rc` is **not expressible**: it needs a value that copies *and* names an allocation, and every value here is one or the other. Three ways of writing it are three fixtures, refused by three unrelated rules. `Gen` is a library, and is `examples/slab/` |
| [#28](https://github.com/alpibrusl/lex-sys/pull/28) | [Tuples](tuples.md) | The first feature a **library** asked for rather than a design — and it costs nothing: a tuple and the struct it replaces emit byte-identical objects |
| [#29](https://github.com/alpibrusl/lex-sys/pull/29) | [Shadowing](shadowing.md) | The restriction was a real rule stated too bluntly. A binding may be shadowed exactly when it is dead — which is the rule assignment already had, so it is one rule with two syntaxes |
| [#30](https://github.com/alpibrusl/lex-sys/pull/30) | [Standard input](standard-input.md) | Not a seventh capability: a second **label** on `Io`, following `Fs`'s `fs_read`/`fs_write`. Which renamed `putchar`'s effect to `io_write` across the repository |
| [#31](https://github.com/alpibrusl/lex-sys/pull/31) | [Modules](modules.md) | **A module reaches no hash.** A call has encoded the callee's hash rather than its spelling since M0, so namespaces cost the identity system nothing — and a module is not a trust boundary: `pub` means reachable, never safe |
| [#32](https://github.com/alpibrusl/lex-sys/pull/32) | [A standard library](standard-library.md) | `--std`, with the source compiled into the binary rather than looked up. Found the compiler emitting **every** non-generic function rather than what `main` reaches — 6720 bytes against 1048 for a program calling none of it |
| [#33](https://github.com/alpibrusl/lex-sys/pull/33) | [Mode polymorphism](mode-polymorphism.md) | Checking a claim found a **leak and a double free**: a `val` on a generic declaration was trusted rather than checked. And a worse bug — a call took its types from one function and its effect row from another, so a call into C could declare `[]` and compile |
| [#34](https://github.com/alpibrusl/lex-sys/pull/34) | [Collections](collections.md) | Which collections hold a resource is decided by **shape**, not generics: a list works because taking it apart produces its elements; an array does not because freeing one is a single `free` that runs nothing |
| [#35](https://github.com/alpibrusl/lex-sys/pull/35) | The README | Not a language change: the README had grown a paragraph per slice in three places at once, so the history moved here and the per-example prose moved to `examples/` |
| [#36](https://github.com/alpibrusl/lex-sys/pull/36) | [A borrowed field](reading-references.md) | The double free was never about **reading**, it was about **owning**. A `res` field through a reference is a borrow, and the refusal that used to guard it was doing work the type rule already does |
| [#37](https://github.com/alpibrusl/lex-sys/pull/37) | [Slicing](slicing.md) | `s[a..b]`, and the answer to "a writer abstraction": there should not be one, because a function taking a `Writer` must declare the **union** of what every destination could do, and a union row is not an exact row. The abstraction is the buffer |
| [#38](https://github.com/alpibrusl/lex-sys/pull/38) | [`defer`](defer.md) | Sugar that **stays** sugar: expanded during lowering, so the checker replays the same events the hand-written version would, and there is one set of linearity rules rather than two. §12's question answered — "visible" here means *the type says what happened*, not "written on the line where it runs" |
| [#39](https://github.com/alpibrusl/lex-sys/pull/39) | [The authority surface](authority.md) | §12's release question answered **no**: the four `release` calls are not ceremony around the authority declaration, they *are* it — `main` owns rather than borrows, so its row is `[]` and every entry point has the same signature. `lex-sys authority` reads them instead |
| [#40](https://github.com/alpibrusl/lex-sys/pull/40) | [`[budget]`](budget.md) | Answered **no**, from the units: a budget is wall-clock seconds, commands and **cents**, none of which is a property of a program's text. `lex-os` already charges it, at the boundary that can stop you. What was wanted was legibility, so the authority report grew `--output json` |
| [#41](https://github.com/alpibrusl/lex-sys/pull/41) | [What a program can reach](reach.md) | Answered by building it: a **REST endpoint over a real TCP socket**, with no socket type, no `Net` capability and no library — because sockets are libc and libc has a name. The no's are one sentence: a foreign *result* is a scalar, so every opaque handle (TLS, libpq, `FILE *`) is out. And the row cannot say `net`, because a library is not an authority domain |
| [#42](https://github.com/alpibrusl/lex-sys/pull/42) | [What the overflow trap costs](overflow-cost.md) | The README's *"low single-digit percent"* had never been measured. Measured: +2.8% call-bound, +3.6% memory-bound, **−9.1%** branch-bound, **+40.5%** arithmetic-bound — so not a percentage but a rule. And the stated reason was wrong: not the never-taken branch, but that **a trap is observable, so the loop cannot vectorise** — clang pays 46% and gcc 74% for the same guarantee. Both documents corrected in place |
| [#43](https://github.com/alpibrusl/lex-sys/pull/43) | [Bits](bitwise.md), and [a port](porting.md) | The first program here that **already existed**: coreutils `base64`, byte-for-byte on both directions. It needed the bit operators and hex literals and *nothing else* — no capability, no library, no change to linearity or rows. And running the suite under load for an unrelated reason found a real bug in `examples/serve/`: one `read` returns what arrived, not what was sent |
| [#44](https://github.com/alpibrusl/lex-sys/pull/44) | [A port with resources](porting.md#9-the-second-port-sort) | `LC_ALL=C sort`, five owned resources on the heap, checked against GNU. Answered §6's four untested things: the move loop costs three tokens rather than difficulty, effects concentrate at the edges (four of eight rows are `[]`), and `borrow mut` never got in the way — five nested blocks did. Found **four** missing library functions, every one absent because nothing had asked |
| [#45](https://github.com/alpibrusl/lex-sys/pull/45) | [Against C and Rust](against-c-and-rust.md) | The measurement `overflow-cost.md` §4 said was owed: **1.6× at equal semantics**, on both a compute-bound and a memory-bound kernel, with Rust within 4% of C — so the gap is the backend, not ownership. And the numerical row: f64 is **17% slower** than the fixed point lex-sys is forced into, so the missing `float` costs precision (10 orders of magnitude) rather than speed |
| [#46](https://github.com/alpibrusl/lex-sys/pull/46) | [What a checked row is worth](purity.md) | The answer to *is there anything it does better?* — **yes, exactly one thing**: the row is a checked purity proof, which C can only promise unchecked and Rust cannot state. 35% of functions here qualify; worth **1.94×** as CSE and **158×** with hoisting. Collected by nothing: Cranelift has no call attribute, and an own optimiser is a non-goal — now an *informed* one |
| [#48](https://github.com/alpibrusl/lex-sys/pull/48) | [Floating point](floating-point.md) | `float`, IEEE-754 binary64 in full. The interesting decision is §2.1: NaN and infinity do **not** trap, because wrapping lies about a value where NaN announces the absence of one — and a per-operation trap would defeat exactly the loops floats are for. `truncate` traps on what C leaves undefined. Writing it found a real collision the design had waved away: `t.0.1` |
| [#49](https://github.com/alpibrusl/lex-sys/pull/49) | [Printing a float](float-printing.md) | The shortest decimal that reads back to the same bits, and it is **library code**: `std/fmt.ls` is written in lex-sys, row `[]`, no `Heap`, and the compiler's whole contribution is `bits_of` — one bitcast. `std.bignum` underneath has no division, because the one quotient a printer needs is a single digit and nine subtractions settle it. Checked against Rust's `{:e}` over 9000 values, which found two real defects an example never would; §3.4 records the three where Rust and Python disagree and why both are right. A comment claiming 2200 bits was measured at 1080 and corrected |
| [#50](https://github.com/alpibrusl/lex-sys/pull/50) | [Compile-time evaluation](compile-time.md) | The answer to *can it be faster than C?* began with finding we were **slower at the easiest thing there is**: `2 + 3 * 4 - 14` is zero and emitted fifteen instructions, because a checked add is `sadd_overflow` + `trapnz` and Cranelift's folding rules are written for the plain form — `overflow-cost.md` §3.2's mechanism sighted a second time. Folding it in the front end brings that to parity; a **certain trap is now a compile error** rather than a `SIGILL`. Past C in one place only: clang gives up on recursion, so `fib(23)` is a runtime call at `-O2` and a constant here. And the estimate that motivated the work was wrong — 65 foldable calls predicted, **15** actual, because pure-and-constant is necessary and not sufficient |
| [#51](https://github.com/alpibrusl/lex-sys/pull/51) | [Compile-time data](compile-time-data.md) | A `static` item: a body with no parameters and no run time, evaluated during compilation into read-only data. `examples/base64/` drops its 64-entry per-character scan and decodes **6.1×** faster. But the measurement corrected the pitch: `compile-time.md` §8 called this "the one that pays" and **5.7× of the 6.1× was already writable** in 36 lines of plumbing. What the feature actually buys is that plumbing, read-only pages, and the row that is a capability argument — 65 536 entries does not fit in a 64 KiB arena and **traps**, so a program that released `heap` could not have one at all |

### The pattern, if there is one

Seventeen of these twenty-five slices found a bug, falsified a claim the
project had already written down, or both — and three of those were
soundness bugs reachable from ordinary code. That is not an accident of luck: each
slice is built by writing the thing the previous document said was
possible, and the documents keep being wrong in the same direction —
optimistic about what generality the type system already had.

The convention that follows is worth stating, because it is why the
documents are trustworthy at all: **a falsified claim is corrected in
place, in the document that made it, rather than quietly edited.**
`sharing.md` corrects `linearity-and-effects.md` §9;
`collections.md` corrects both `standard-library.md` §4 and
`mode-polymorphism.md` §1, the second of which was itself a correction;
and #36 corrects `reading-references.md` §2.0 and `sharing.md` §2.1,
where a refusal turned out to be broader than the hazard it was written
for; and #41 corrects a refusal that named `()` as a foreign result, in a
language whose grammar refuses `()` on purpose — two correct rules with a
loop between them, which only a program walking into it would find; and
#42 corrects `README.md`'s performance expectation and
`defined-behaviour.md` §2.1, which had put a number on the overflow trap
that nobody had ever measured — the one claim here that an outside reader
caught before the project did.

---

## What is next

| Next | Why it is next |
|---|---|
| Does `&!` mean unique? | The README's *"stronger aliasing facts than `&mut`"* was false: `both(s, s)` compiles, so two unique references can be one object and no `noalias` can be emitted. Making `&!` mean what `&mut` means would reach Rust's position — parity with C, via Fortran's oldest advantage — and it refuses programs that compile today, so it is a language decision rather than a pass |
| Struct layout | Every leaf costs 8 bytes, so `struct Rgb { r, g, b: byte }` is 24 where C's is 3 — measured, 2730 per 64 KiB arena. Nothing in the language can observe layout (no raw pointers, no `offsetof`, and `reach.md` §3.2 keeps aggregates out of FFI), so the compiler may pack, reorder **and** transpose. Packing reaches C; array-of-structs to struct-of-arrays goes past it, on the memory-bound code where the backend gap is smallest |
| `std.math` over floats | `sqrt`, `sin`, `exp` — each either a libc call gated by `Ffi`, which would make arithmetic need a capability, or an implementation with its own error analysis. The capability question comes first |
| File handles | `porting.md` §9.1 put a program behind `filesystem.md` §3's own deferral. Reading a file of unknown size currently means reading it repeatedly — 1.2 MB is read six times — because `fs_read` cannot report truncation and there is nothing to hold open. §3 says a handle is "a milestone, not a paragraph", and it is the milestone a real program is now waiting on |
| Effect polymorphism | A function generic over the *row* it performs. Named nowhere yet, and much larger than anything above |

An LLVM backend is **not** next, and `purity.md` §4.2 is why the case for
it shrank rather than grew: the row's advantage needs a compilation
boundary, lex-sys compiles whole programs, and LLVM infers the same fact
itself when it can see every body. It remains the answer to the 1.6×, and
that is a performance problem in a language that is not yet usable.

Ordinary work, blocked by nothing: `jo` instead of `seto`/`test`/`jne`
(`overflow-cost.md` §3.4), flag parsing, standard error — which `reach.md` §6 upgrades from an omission to a named gap,
since `std.math` does not yet mean what its name suggests.

Environment variables moved off that list and onto `reach.md`'s: `getenv`
returns a `char *`, so they are not reachable through FFI at all and want
a capability with builtins, the way `Args` has.

---

## Deliberately excluded

Borrow checker, traits, `comptime`, an own optimiser, incremental
compilation, LSP, async.

Each is "yes, later" rather than "no". Saying yes early is what turns
three months into three years, and the
[non-goals](../README.md#explicit-non-goals) say which of them are "no"
outright.

---

## Beyond

The first real target is **`lex-os`** — production systems work, no
rewrite risk.

Self-hosting the lex-lang toolchain stays a *spike before a plan*: port
`lex-ast`/`lex-vcs` canonical forms and verify byte-identical
`OpId`/`SigId`/`StageId` over the existing ~136k-op corpus, then decide.
