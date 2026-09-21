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

### The pattern, if there is one

Ten of these seventeen slices found a bug, falsified a claim the project
had already written down, or both — and three of those were soundness
bugs reachable from ordinary code. That is not an accident of luck: each
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
for.

---

## What is next

| Next | Why it is next |
|---|---|
| `[budget]` | Carried over from Lex and not specified here. Plainly a capability carrying an integer; what it costs at runtime, and whether it is checked or merely accounted, is unanswered |
| Effect polymorphism | A function generic over the *row* it performs. Named nowhere yet, and much larger than anything above |

Ordinary work, blocked by nothing: flag parsing, environment variables,
standard error.

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
