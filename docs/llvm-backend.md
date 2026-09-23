# A second backend, and how it would actually get built

> **Status: settled, not built.**
>
> `backend-limits.md` made the case: three separate performance
> arguments — `aliasing.md`'s unspent uniqueness fact, `purity.md`'s
> 158×, `check-cost.md`/`gpu.md`'s missing vectoriser — turn out to be
> one argument, because they are all blocked by the same four-line
> Cranelift predicate. This document is the next question, which nobody
> had asked yet: **how**, concretely, would a second backend be wired
> in, and what does building one actually cost before it computes a
> single one of those facts?
>
> Two things came out of spiking it rather than guessing. The
> architecture is cheaper than it looks: textual LLVM IR, `clang` as an
> external tool, no new build dependency — `connect.md` §9's three-slice
> split, aimed at a new target instead of a new capability. And there is
> a real, measured correctness trap sitting in the middle of it: the
> obvious way to emit lex-sys's overflow trap in LLVM IR produces the
> **wrong signal**, silently, on one of the two targets this project
> ships.

---

## 1. The question this document answers

`lib.rs`'s own header has said this since M0:

> *"Cranelift rather than LLVM for M0 (#3): trivial to embed, no C++
> build dependency, and compile speed suits the feedback loop. LLVM
> arrives later for release-quality codegen — the plan is to ship both,
> as rustc does."*

"Ship both" has been the plan for over ninety pull requests and nobody
had written down what that means in practice: one compiler, two
backends, selected how, sharing what, and — the part every one of this
project's other big features gated on — what is the smallest thing that
proves the wiring works before either backend has to be feature-complete
against the other.

That smallest thing is what §3 measures.

---

## 2. Two ways to talk to LLVM, and why one of them is the one M0 already rejected

**Option A: link against LLVM.** `inkwell` or `llvm-sys`, building IR
through a Rust API the way `cranelift-codegen` is used today. This is
what most Rust-hosted LLVM front ends do.

**Option B: emit LLVM IR as text, and shell out to `clang`.** The
compiler writes a `.ll` file; an external process turns it into an
object file; linking is whatever already happens to that object file.

Option A is exactly the C++ build dependency `lib.rs`'s M0 comment
named as the reason to wait: `llvm-sys` links against a specific major
version of `libLLVM`, found via `llvm-config` on the host, and getting
that right across `ubuntu-latest` and `macos-latest` — two different
package managers, two different default LLVM versions, neither
guaranteed to match what `inkwell`'s pinned version wants — reintroduces
the "no C++ build dependency" §1 already quoted from `lib.rs`, just
deferred from the compiler's *language* to the compiler's *build*.

Option B has no such coupling, and it is not a new pattern here: `link`
in `crates/lex-sys/src/main.rs` already shells out to `cc` for every
single build, Cranelift path included (`docs/backend-limits.md` never
had to mention this because it is not a Cranelift fact, it is a M0
fact). A second backend that shells out to `clang` for both compiling
*and* linking is the same architecture the first backend already has,
aimed at one more external tool.

**Decided: Option B.** §3.1 is why it is cheaper than it sounds.

---

## 3. The spike

Two questions had to be answered before either was a claim: does a
plain `.ll` file actually compile on both targets without extra
tooling, and does the one piece of codegen this language cannot get
wrong — the overflow trap — come out the same way.

### 3.1 `clang` alone is enough; `llc` and `opt` are not needed

The concern going in was that LLVM IR needs `llc` (to lower `.ll` to an
object file) and optionally `opt` (to run passes), neither of which
ships with Xcode's command-line tools — only Homebrew's separate `llvm`
formula has them (confirmed: `/usr/bin/clang` is Apple's, and `llc`
is not on `PATH` without `brew install llvm`).

It does not matter. `clang` itself accepts `.ll` as an input language
and both compiles and optimises it directly:

```
$ /usr/bin/clang -c hello.ll -o hello.o   # Apple's clang, no Homebrew LLVM installed
$ /usr/bin/clang hello.o -o hello
$ ./hello
hi!
$ echo $?
42
```

`clang -O2 -c foo.ll -o foo.o` is the entire codegen step. `ubuntu-latest`
and `macos-latest` both ship `clang` already (`ci.yml`'s existing `cc`
step already depends on this), so the CI matrix needs **no new
installed tool** for this backend — a smaller footprint than the
current one, which the `no C++ build dependency` reasoning did not even
promise.

The one thing worth carrying into the implementation: `clang` warns
(`overriding the module target triple`) if the emitted `.ll` omits or
disagrees with the host triple. Harmless here, but the real lowering
should emit `target triple = "..."` from `compile_object_for`'s own
`Triple` argument rather than let `clang` guess it — the same value
Cranelift's path already threads through today.

### 3.2 The overflow trap: the obvious spelling is wrong, and it is wrong silently

Every one of `overflow-cost.md`, `emitted-checks.md` and
`backend-limits.md` treats `SIGILL` as the signal a checked trap raises,
because that is what Cranelift's `trapnz` lowers to. The obvious LLVM
translation of "add with overflow, then trap" is:

```llvm
%r = call {i64, i1} @llvm.sadd.with.overflow.i64(i64 %a, i64 %b)
%ov = extractvalue {i64, i1} %r, 1
br i1 %ov, label %trap, label %ok
trap:
  call void @llvm.trap()
  unreachable
```

Measured on darwin-aarch64: this compiles and runs, and the shell
reports exit **133 — `SIGTRAP`, not `SIGILL`**. `llvm.trap()` lowers to
`brk #0x1` on AArch64 (confirmed by disassembling the compiled output),
which the kernel delivers as a breakpoint trap. The actual compiler's
own output, for the same overflow, disassembles to `udf #0xc11f`, and
the shell reports 132 — `SIGILL` — every time.

This is a real gap the existing suite would not have caught: every
runtime-trap test here (`compile_time.rs`, `arguments.rs`, and any other
`Command::status.code()`) checks `assert_eq!(run.status.code(), None, …)`
— Rust's own signal for "a signal killed this," true for `SIGTRAP` and
`SIGILL` alike — and none reads `std::os::unix::process::ExitStatusExt`'s
`.signal()` to see *which* one. A backend that emitted `SIGTRAP` here
would pass every test in this repository today while contradicting what
`docs/defined-behaviour.md` and `emitted-checks.md` say the language
does, and only a person watching a shell's `$?` would ever see the
difference. §5's second slice is also where that gap gets closed.

The fix, also measured: emit the trap as target-specific inline
assembly instead of `llvm.trap()` —

```llvm
trap:
  call void asm sideeffect "udf #0xc11f", ""()   ; aarch64
  unreachable
```

— which disassembles to the same `udf #0xc11f` Cranelift emits and
exits 132, matching exactly. The x86-64 side needs the equivalent
`"ud2"` string, unverified on this machine (no x86-64 host in this
session) but consistent with `ud2` being the documented cause of
Cranelift's own `SIGILL` there (`emitted-checks.md` §4).

**This is the finding worth the whole spike.** A backend that reached
for the portable intrinsic — the *correct-looking* choice, and the one
an LLVM newcomer would pick — would have shipped a language whose
defined-behaviour guarantee (`docs/defined-behaviour.md`) is silently
weaker under one backend than the other, on one target, and nothing in
the existing test suite would have noticed. Trap **codegen** cannot be
generic between targets; only the checker's decision to trap can be.

### 3.3 The traps were already asymmetric between targets, before this document existed

Checking §3.2 against `int::MIN / -1`-style division found something
`emitted-checks.md` had only ever measured on x86-64. On aarch64
(measured here, `div0.ls` and `mod0.ls` built with the actual
compiler): **both** division and remainder by zero exit 132, `SIGILL`,
disassembling to explicit `udf` after a compare. `emitted-checks.md` §4
found, on x86-64, `1/0` dies with `SIGILL` from its own `ud2` and `1%0`
dies with `SIGFPE` **from the hardware** — a different pairing of
operator to signal.

The reason is the hardware, not Cranelift's choice: x86-64's `idiv`
faults on division by zero by itself (`#DE`, delivered as `SIGFPE`), so
Cranelift only needs an explicit check for the one case the instruction
does not cover (`int::MIN / -1`, which does not trap in hardware but
must). AArch64's `sdiv` does not fault at all — ARM's architecture
manual defines division by zero as answering 0 — so Cranelift must
check *both* zero and `int::MIN / -1` explicitly on that target, and both
checks reach the same `udf`.

**A second backend inherits this, not a cleaner version of it.** The
observable contract (`docs/defined-behaviour.md`) is target-independent
prose — "traps" — but which signal that trap is *is* target-dependent,
already, in the compiler this one has to match. Getting this right is
per-target codegen work, checked instruction by instruction the way
`emitted-checks.md` checked Cranelift, not a single IR pattern reused
across `isa/`.

---

## 4. The shape this takes

A sibling crate, `lex-sys-codegen-llvm`, with the same doorway
`lex-sys-codegen` has:

```rust
pub fn compile_object_for(
    program: &Program,
    entry: &str,
    triple: Triple,
) -> Result<Vec<u8>, CodegenError>
```

fed the identical `lex_sys_ir::Program` the Cranelift path already
consumes — nothing about parsing, checking, or lowering to IR changes.
`lex-sys-ir/src/lib.rs`'s own header already states the boundary this
relies on: *"the backend receives IR that cannot fail."* Internally it
builds a `.ll` string and shells out to `clang -c` (§3.1), the way
`link` already shells out to `cc` (§2).

The CLI grows one flag, `--backend cranelift|llvm`, defaulting to
`cranelift` — so this is purely additive. This is not an edition:
nothing about the *language* changes, only which tool compiles it. But
it is the same shape `docs/editions.md` §6's rule takes for the
language — "only additions are held open" — applied to the compiler
instead: a second backend that changes no existing default is exactly
an addition. Every existing test, example and fixture keeps running
exactly as it does today; the new backend is opt-in until §5's bar is
cleared enough to flip a target's default, which is a much later
decision than this document makes.

`link` itself needs **no change at all**: it already takes an object
file and a `cc`, and an LLVM-produced object file is not distinguishable
from a Cranelift-produced one at that step.

---

## 5. What the first real slice is, and is not

Matching `connect.md` §9's own precedent — `Net` turned out to be
bigger than one slice too, and was "agreed as three slices once that
came into view" rather than forced into one PR:

**First slice: the doorway and the smallest possible program.**
`lex-sys-codegen-llvm` exists, takes `--backend llvm`, and
`examples/hello.ls` — the exact program `ci.yml`'s smoke test already
builds — produces identical stdout through it. This needs: function
declarations and calls, `int`/`Io`/capability erasure (already erased
by the time IR reaches a backend, so this should cost nothing extra),
`putchar`, and process exit. No arithmetic, no traps, no structs.

**Second slice: checked arithmetic, on both targets.** Every `BinOp`
that can trap (`Add`, `Sub`, `Mul`, `Div`, `Rem`, `Shl`, `Shr`), checked
against `benches/guards.c`-style kernels the way
`every_guard_kernel_agrees_across_its_three_modes` already checks
Cranelift's three modes against each other — extended to check the
*signal* a trap raises, not only whether one fired, which §3.2 found
nothing currently does. This is where §3.2's fix and §3.3's per-target
asymmetry actually get implemented and pinned down as tests, on real
CI hardware for both targets rather than one machine's disassembly.

**Later slices, each its own PR, not scoped further here:** structs and
enums (LLVM's own aggregate types, a more direct mapping than
Cranelift's flattened layout — worth its own measurement rather than an
assumption), arenas and regions (a `Heap`-free allocation scheme LLVM
has no special vocabulary for either, so likely the same bump-pointer
strategy translated rather than redesigned), `Ffi`/`extern fn` (LLVM's
own `declare` is close to a direct match), `Net`. Each is a `Builtin` or
`BinOp` variant this document is not claiming to have scoped — `ir.rs`
has 34 builtins and lists them so the next slice can pick a subset by
reading them rather than guessing at the size of what is left
(`crates/lex-sys-ir/src/builtin.rs`).

**Explicitly not this document's to answer: is it faster.**
`backend-limits.md` §4 already warned against overclaiming what LLVM's
vocabulary buys — the ceiling is lower than the sum of the three
numbers, because `check-cost.md` and `poison.md` already found real
limits on what a vectoriser rescues. Measuring the actual ratio against
Cranelift wants a backend that is *correct* first; a speed number from
an incomplete backend would be exactly the kind of claim
`benchmarks-game.md` §2.1 was corrected for making before it measured.

---

## 6. Open

| Question | Why it waits |
|---|---|
| The exact `ud2`/`udf` spelling and signal on linux-x86_64 | §3.2's fix is verified on darwin-aarch64 only — no x86-64 host in this session. First slice's CI, on real hardware, is the check |
| Which `clang`/LLVM IR version to target | Textual IR has version-dependent syntax; pinning to whatever `ci.yml`'s two runners ship, rather than a specific LLVM release, is the plan until a real incompatibility forces a choice |
| Optimisation level and its effect on trap codegen | `clang -O2` might fold or reorder a checked operation in a way `-O0` would not; `check-cost.md` and `poison.md`'s Cranelift findings do not transfer automatically, and this needs its own measurement once arithmetic lowering exists (§5's second slice) |
| Whether `noalias`/purity attributes get emitted at all | `backend-limits.md` §4's ceiling question — real, and not this document's to answer before the backend can run anything |
