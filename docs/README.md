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
| `memory-model.md` | Regions/arenas, what escapes, the escape hatches (refcount / generational refs) and their runtime cost. | not written — §5 and §6 settled regions, arenas and escape, `heap.md` settled the general heap, and `sharing.md` has now settled §9's hatches. Nothing is left that a document of its own would say |
| `strings.md` | What a string is: bytes rather than an encoding, `byte` as storage rather than arithmetic, packed `[byte]` layout, literals and the static region, and what crosses to C. | **settled and built** — gated M3's last item, and §9's must-reject suite is enforced fixture by fixture |
| `boxed-slices.md` | `Box[[T]]`: the second shape a box comes in, a pointer *and* a length, and the three operations a run of heap values needs. The foundation every collection wants. | **settled and built** — §7's must-reject suite is enforced, and `examples/buffer/` is the growable buffer it unblocks |
| `many-files.md` | A program in more than one file: named on the command line, one flat namespace, identity by content rather than location, and why spans became global. | **settled and built** — the precondition for a library of any kind; §8's cases are enforced by conformance tests |
| `arguments.md` | The `Args` capability: why reading argv is an effect at all, `arg_count` / `arg`, and why an argument is a shared `&static [byte]`. | **settled and built** — the "command-line" half of M3's acceptance criterion; §7's must-reject suite is enforced fixture by fixture |
| `reading-references.md` | Reading a value through a reference: `*r`, and `match` on a reference binding payloads as references. One rule — a reference gives references. | **settled and built** — closed the two limits `heap.md` §3.0 and §4.1 left open; §7's must-reject suite is enforced fixture by fixture |
| `heap.md` | The `Heap` capability and `Box[T]`: one value one allocation, why the heap cannot leak, recursive types, and heap versus arena. | **settled and built** — closes M2's last unchecked item; §8's must-reject suite is enforced fixture by fixture |
| `filesystem.md` | The `Fs(prefix)` capability, why the file operations are builtins rather than `extern fn`, the runtime path check and why `..` is refused rather than normalised. | **settled and built** — the last mile to M3's acceptance criterion; §7's must-reject suite is enforced fixture by fixture |
| `sharing.md` | The two escape hatches of §9 as built: why `Rc` is **not expressible** without a copyable pointer, why `Gen` is, and what a linear library costs to write. | **settled and built, and it corrects `linearity-and-effects.md` §9** — three reject fixtures for the three ways `Rc` fails, and `examples/slab/` for the one that works |
| `tuples.md` | `(A, B)`: an anonymous struct with positional components, and the first **structural** type here — no declaration, so two files agree on one without either declaring it. Mode is computed rather than declared. | **settled and built** — the first feature whose case was made by a library rather than a design (`sharing.md` §4); §8's must-reject suite is enforced fixture by fixture |
| `shadowing.md` | Shadowing within a block: why it was refused, and the rule that replaces the refusal — a binding may be shadowed exactly when it is dead, which is the rule assignment already had. | **settled and built** — the last of `sharing.md` §4's three gaps; §7's suite is enforced, and one must-reject fixture was retired to it |
| `standard-input.md` | Reading the console: `getchar`, and why it is not a seventh capability but a second **label** on `Io` — which renames `putchar`'s `io` to `io_write`, following `Fs`'s `fs_read`/`fs_write`. | **settled and built** — the one I/O direction the language did not have; `examples/tally.ls` is `wc` over a pipe, and §7's suite is enforced |
| `modules.md` | `module`, `import`, `pub`, and qualified names. A module is a **namespace, not an identity**: it reaches no hash, which is a property of content-addressing rather than luck. Not a trust boundary — `pub` means reachable, never safe. | **settled and built** — the precondition for a standard library; §8's suite is enforced, and `moving_a_function_into_a_module_changes_no_hash` checks the central claim |
| `defined-behaviour.md` | Every place C/Rust leave behaviour open, and what we define it to instead. Integer overflow, evaluation order, layout. | **written and enforced** — overflow traps, evaluation order is left to right everywhere, and §9 lists the fixture behind each rule. §8 is what does not exist yet, which is absent rather than undefined |

`linearity-and-effects.md` was the gating artifact: the decision set that
determined whether this is a three-month prototype or a three-year project.
It was settled before M2 started, its must-reject list was read as what it is
— the M2 conformance suite, stated in advance — and M2 was then built against
it slice by slice. Every rule in it now has a fixture, and the two places the
implementation had to decide something the document left open (§3.1's
instantiation rule, §6's unique-to-shared coercion) are written back into it
rather than living only in code.
