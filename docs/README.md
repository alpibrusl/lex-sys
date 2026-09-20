# Design documents

Design lands here before the code that implements it, which is the cheap
place for it to be wrong. M0, M1 and M2 are built; `bootstrap.md` records
what M0 settled, and `linearity-and-effects.md` carries a "what was built"
note in every section whose code exists.

| Doc | Purpose | Status |
|---|---|---|
| `linearity-and-effects.md` | The core type-system rules: linear/affine ownership, capability-typed effects, how they unify, and the cases that **must** be rejected. Worked examples throughout. | **settled and built** ([#2](https://github.com/alpibrusl/lex-sys/issues/2)) — §3 through §8 implemented, every row of §11's must-reject table enforced by a fixture |
| `bootstrap.md` | What M0 settled to exist: bootstrap host language, file extension, layout, the M0 surface, what is scaffolding and what replaces it. | written ([#3](https://github.com/alpibrusl/lex-sys/issues/3)) |
| `canonical-ast.md` | AST shape, canonicalisation rules, per-unit identity (signature vs body hashing), and the determinism invariants. | **written** — implemented by `lex-sys-id`; `lex-sys ids <file>` prints them. §8 lists what is not yet a contract |
| `memory-model.md` | Regions/arenas, what escapes, the escape hatches (refcount / generational refs) and their runtime cost. | not written — §5 and §6 settled regions, arenas and escape, and `heap.md` has now settled the general heap. What is left is §9's *sharing* hatches, `Rc` and `Gen`, which are libraries and need a module system first |
| `strings.md` | What a string is: bytes rather than an encoding, `byte` as storage rather than arithmetic, packed `[byte]` layout, literals and the static region, and what crosses to C. | **settled and built** — gated M3's last item, and §9's must-reject suite is enforced fixture by fixture |
| `reading-references.md` | Reading a value through a reference: `*r`, and `match` on a reference binding payloads as references. One rule — a reference gives references. | **settled and built** — closed the two limits `heap.md` §3.0 and §4.1 left open; §7's must-reject suite is enforced fixture by fixture |
| `heap.md` | The `Heap` capability and `Box[T]`: one value one allocation, why the heap cannot leak, recursive types, and heap versus arena. | **settled and built** — closes M2's last unchecked item; §8's must-reject suite is enforced fixture by fixture |
| `filesystem.md` | The `Fs(prefix)` capability, why the file operations are builtins rather than `extern fn`, the runtime path check and why `..` is refused rather than normalised. | **settled and built** — the last mile to M3's acceptance criterion; §7's must-reject suite is enforced fixture by fixture |
| `defined-behaviour.md` | Every place C/Rust leave behaviour open, and what we define it to instead. Integer overflow, evaluation order, layout. | **written and enforced** — overflow traps, evaluation order is left to right everywhere, and §9 lists the fixture behind each rule. §8 is what does not exist yet, which is absent rather than undefined |

`linearity-and-effects.md` was the gating artifact: the decision set that
determined whether this is a three-month prototype or a three-year project.
It was settled before M2 started, its must-reject list was read as what it is
— the M2 conformance suite, stated in advance — and M2 was then built against
it slice by slice. Every rule in it now has a fixture, and the two places the
implementation had to decide something the document left open (§3.1's
instantiation rule, §6's unique-to-shared coercion) are written back into it
rather than living only in code.
