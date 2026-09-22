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
| [#52](https://github.com/alpibrusl/lex-sys/pull/52) | [Layout](layout.md) | The first slice here to build **almost nothing**, on purpose. This roadmap said packing reaches C and transposing goes past it; measurement says the second half is wrong. Transposing is worth **1.43×** in lex-sys and **1.31×** in C — the same transform, the same size, so it moves both along rather than closing a gap, and the vectorised half of the prize needs a vectoriser `overflow-cost.md` §3.2 already showed we do not have. Packing is real (2.6× on a byte-heavy struct) and **3 of 83** struct fields here are `byte` or `bool` — and `slab`'s `Entry` would not shrink at all. So: `lex-sys layout`, which makes the deferral falsifiable instead of remembered |
| [#53](https://github.com/alpibrusl/lex-sys/pull/53) | [Against a wider set](benchmarks-game.md) | Three programs from the Computer Language Benchmarks Game — fannkuch-redux, spectral-norm, binary-trees — each checked against the answer the Game publishes. The **1.6×** headline was two kernels' midpoint; across five it is **1.17× to 2.58×**, and it tracks how much of the run is in code Cranelift generated. Porting found a **compiler crash**: two *sibling* `region` blocks put the backend's arena stack at length 0 where index 1 was wanted, latent since arenas landed because nothing here had two regions side by side. And `scripts/game.py` reports the **spread**, which no published ratio here had carried |
| [#54](https://github.com/alpibrusl/lex-sys/pull/54) | [Bulk output](bulk-io.md) | `benchmarks-game.md` §2.1 parked this as "a real finding and a separate slice", and the finding is not the **12.8×**. `examples/serve/` had been writing whole slices for four slices, through `Ffi("libc")` — which `reach.md` §5 says is every authority at once. So a program that wanted to print quickly had to ask for the filesystem, the network and `exec`, and a program that asked only for `Io` was held to one byte per call: **the cheap thing to grant was the expensive thing to run**, which is the wrong lesson for a capability language to teach every time someone profiles. Fixed in the primitive rather than the library, because a function holding `&!i Io` can only call what `Io` authorises. It lowers to `fwrite` and not POSIX `write`, since `putchar` is buffered and a raw descriptor write would interleave wrongly. And §4 declines the headline: base64 gains **1.6×**, not 12.8×, because 6 ms of the saving goes straight back into byte-at-a-time buffer fills that `cc -O2` vectorises away. **§4.1 then corrects §4 itself** (#55): it had said a program "whose time is output" gets most of the 12.8×, and `examples/sort/` — which writes 9 MB and moved onto the bulk path with no change to the program at all — gains **1.22×**. Volume written is not time spent writing, and the gain is only ever the share of the runtime that was call overhead |
| [#55](https://github.com/alpibrusl/lex-sys/pull/55) | [Canonical identity, made observable](canonical-ast.md#8-not-yet-contracts) | Asked what it would take to put lex-sys code in lex-lang's `lex-vcs`, and the answer was that **81%** of that crate is already language-agnostic — it keys on `String` ids and `BTreeSet<String>` effects — while lex-sys's own hashes are the part that is not ready. So this slice builds the falsifier rather than the integration. `lex-sys-id` had 61 tests and **all of them were relational**: they compare two hashes to each other, and not one said what a hash *is*, which left §8's "not frozen across releases" a claim nobody could observe. 35 golden fixtures now pin it, reporting by fixture name so an insertion does not cry wolf — and the first thing they caught was one of my own fixtures: perturbing the `BINARY` tag moved `unary`, because `0 - a` is a subtraction and there is no unary minus, so that fixture had never covered `UNARY` at all. And §8's field-order entry turned out **wrong in both halves**: a struct literal was never a gap, because the checker *refuses* `P { y: 2, x: 1 }` rather than reordering it (`defined-behaviour.md` §3 — the order you read is the order it runs); the destructuring pattern's gap was real but the stated reason was not, since sorting by name settles it with no declaration lookup and also survives a struct reordering its own fields. The binders sort with the names, because they are positional from there on and a collision would be worse than the difference removed |
| [#56](https://github.com/alpibrusl/lex-sys/pull/56) | [UTF-8](utf8.md) | Asked how far lex-sys is from a string type, and the answer was already written: [`strings.md`](strings.md) settled it as `&r [byte]` with no encoding claimed. So the question became the library, and the probe says **the language needs nothing** — a forty-line decoder matched GNU `wc -m` exactly on the first compile (119 bytes, 79 code points over ASCII, Latin, Japanese, emoji and a combining sequence), and `vec.Vec[&t [byte]]` compiles, so `split` can return substrings bound to the text they came from. What the probe did find is the decision `strings.md` §1 said would have to be paid somewhere: §1 declined a validated string because it would have to say what an invalid one *is* — an error, a replacement character, an unrepresentable state — and a **decoder cannot decline**. Seven malformed fixtures, four implementations, **four different answers**, and GNU contradicts itself by counting `f5 80 80 80` as one character though it encodes a value above U+10FFFF. So GNU is not the oracle here, which also means `wordcount.ls` must never be checked against `wc -m`. §3 takes strict validity and the Unicode maximal-subpart skip rule, verified to reproduce Python's replacement counts on every fixture — and `std.utf8` now implements it, agreeing with **Rust's own** `from_utf8_lossy` and `str::from_utf8` over 2,000 generated cases. Nothing in the design moved on contact with the code, which is the first slice here where that is true |
| [#57](https://github.com/alpibrusl/lex-sys/pull/57) | [The rest of the string library](standard-library.md) | [`utf8.md`](utf8.md) §1 said `split`, `trim`, `join`, ordering and case are "code, not design". Rather than invent the surface, `examples/cut/` was ported to **ask** for it — the route `vec.set` and `vec.swap` took — and it asked for exactly two: `bytes.count_byte` and `bytes.field`. `trim` came with the field parser and `bytes.compare` came from `examples/sort/`, which had the rule inline. Four functions, three call sites, checked against GNU `cut` on eight field specs. Porting it found the **64 KiB arena as a line-length limit**: a 65 000-byte line buffer beside a 1 025-byte bitmap traps on exhaustion, so the number in the source is what fits rather than what was wanted, and the comment says so. And it found `standard-library.md` §5.3 claiming a fix it had not made — `wordcount.ls` moved its word boundary to `std.bytes`, **`tally.ls` kept a byte-identical copy**, so "one definition, in one place" was two definitions that happened to agree with nothing checking. Corrected in place, in the entry that named the failure mode |

### The pattern, if there is one

Twenty-one of these twenty-nine slices found a bug, falsified a claim the
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
caught before the project did. And #55 corrects `canonical-ast.md` §8 —
the section whose whole job is to say what is *not* a contract — which had
described a struct literal's field order as a gap it does not have and
given the wrong reason for the one gap it did have. A wrong entry there is
worse than most, because it is the entry everyone else reads to find out
what they may rely on.

---

## What is next

| Next | Why it is next |
|---|---|
| Does `&!` mean unique? | The README's *"stronger aliasing facts than `&mut`"* was false: `both(s, s)` compiles, so two unique references can be one object and no `noalias` can be emitted. Making `&!` mean what `&mut` means would reach Rust's position — parity with C, via Fortran's oldest advantage — and it refuses programs that compile today, so it is a language decision rather than a pass |
| A vectoriser, or a backend with one | [`layout.md`](layout.md) §3 and [`overflow-cost.md`](overflow-cost.md) §3.2 turn out to be the same wall wearing two hats: a trapping add is not reassociable, so the loop does not vectorise, so neither contiguity nor reassociation can be spent. [`bulk-io.md`](bulk-io.md) §4 puts a third hat on it — a byte-at-a-time slice copy costs 6 ms per 8 MB here and nothing measurable in C, which is now the largest single thing between `base64/` and coreutils. It is the one thing that would move several of these numbers at once |
| `std.math` over floats | `sqrt`, `sin`, `exp` — each either a libc call gated by `Ffi`, which would make arithmetic need a capability, or an implementation with its own error analysis. The capability question comes first |
| File handles | **Designed — [`file-handles.md`](file-handles.md).** `porting.md` §9.1 put a program behind `filesystem.md` §3's own deferral, and measuring it moved the argument: the repeated reading costs **2.75×**, not the six times §9.1 implied, but `examples/sort/` turns out to **refuse a file of 8 MiB or more** and to report that exactly like a missing file. Two of the three questions §3 called a milestone are already answered by machinery that exists — a linear value cannot die at the end of a region, and linearity reaches through a generic enum, so `Result[File, int]` works today. What is left is the three-way read answer `standard-input.md` §3.1 also defers, and one §3 never asked: where the path prefix goes in the effect row, since a program must not look more powerful for having been written better (`bulk-io.md` §3.2) |
| fasta and reverse-complement | [`benchmarks-game.md`](benchmarks-game.md) §2.1's two deferred programs. They were waiting on bulk output and nothing else: an output-bound program written through one `putchar` per byte would have measured libc, not lex-sys. That is now false, so they are ordinary work — but [`bulk-io.md`](bulk-io.md) §4.1 withdraws the prediction that they would show a large gain, since `fasta` computes an LCG step per byte and is nearer `base64`'s 1.59× than §1's loop. Blocked on nothing but network access to the Game's published spec |
| lex-sys code in `lex-vcs` | lex-lang's `crates/lex-vcs` is **81% language-agnostic already** — the op DAG, merge, attestations, intents and signing all key on `String` ids and `BTreeSet<String>` effects, and only ~1,500 lines (`compute_diff`, `diff_to_ops`, `body_merge`) walk a Lex `CExpr`. The gate is two lines from being a trait. What is not ready is this side: §8 above. #55 started measuring how often these hashes move; the design doc is worth writing when that number exists and §8 is nearly empty, not before. One thing to think about first — `body_merge` auto-merges disjoint subtrees, and in a linear language two edits that each typecheck can together move a value twice, so it will conflict far more often here than it does there |
| A line reader | `examples/cut/` and `examples/tally.ls` both read `getchar` into a fixed buffer, and #57 found what that costs: a 64 KiB arena caps a line at what fits beside everything else in it. `std.buffer` on the heap grows, as `examples/sort/` shows, so this is a library function nobody has written rather than a gap — but two programs hand-rolling the same loop is how the last four library functions were found |
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
