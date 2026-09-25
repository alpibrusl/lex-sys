# A second backend, and how it would actually get built

> **Status: first two slices built** (§5, `lex-sys-codegen-llvm`, `--backend
> llvm`). §5 originally named `examples/hello.ls` as the first slice's
> target; building it found that claim false, corrected in place below —
> `hello.ls` needs checked arithmetic, bounds-checked indexing and
> string-literal data, and only the first of those three is built yet.
> `tests/accept/llvm_smoke.ls` and `tests/accept/llvm_arith.ls` are what
> the two slices' own bullet lists actually describe. The second slice
> also found LLVM's `sdiv`/`srem` are **undefined**, not trapping, on the
> two inputs Cranelift traps — a correction below, not assumed going in.
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

Built as such: `CodegenError` itself is not duplicated, only reused —
`lex-sys-codegen-llvm` depends on `lex-sys-codegen` for that one type
(`{ function: Option<usize>, message: String }`, already backend-agnostic)
so the CLI's `internal_refusal` needed no change to accept either
backend's failures. Everything else stays as sibling as the plan said:
no Cranelift type, and no Cranelift call, crosses the boundary.

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

**First slice: the doorway and the smallest possible program — built.**
`lex-sys-codegen-llvm` exists, takes `--backend llvm`, and builds and
runs a program needing exactly: function declarations and calls,
`World`/capability erasure (every capability is a zero-field struct, so
it scalarises to no leaves at all — confirmed rather than assumed, since
`lex-sys-codegen`'s own `abi::leaves_into` has no special case for `Io`,
`Ffi`, `Fs`, `Heap` or `Args`; they fall through to the same struct rule
`Split` does, five zero-leaf fields making a zero-leaf whole), `putchar`,
and process exit. No arithmetic, no traps, no structs with real fields.

**This corrects the row above: that program is not `examples/hello.ls`.**
`ci.yml`'s smoke test was assumed to be the smallest one, unchecked
against what `strings.md` had since added to it. It is not — `write_all`'s
`n = n + 1` and `n < len(s)` are checked arithmetic and a comparison,
`s[n]` is bounds-checked indexing, and `"Hello, world!\n"` is a
string-literal data object, and this slice lowers none of them.
`tests/accept/llvm_smoke.ls` is the program this bullet list actually
describes — two functions, one call between them, four `putchar`s, no
operator anywhere — and it is what `lex-sys-codegen-llvm`'s own test
builds and runs, checked byte-for-byte against the Cranelift path's
output for the same file (`crates/lex-sys/tests/conformance/backends.rs`).
`hello.ls` moves to the second slice below, where checked arithmetic
lands, and needs bounds-checked indexing and string data besides —
closer to a third and fourth slice than a continuation of this one.

Every node this slice does not lower is refused with a located
`CodegenError`, never a panic (`docs/internal-errors.md`): unlike
`lex-sys-codegen`'s `unreachable!`s, which state an invariant the checker
already guarantees, a gap here is an ordinary limit of an opt-in,
unfinished backend, and `--backend cranelift` is unaffected either way
(`--backend` is purely additive, defaulting to `cranelift`).

**Second slice: checked arithmetic — built, and not quite as scoped.**
Every `BinOp` that can trap (`Add`, `Sub`, `Mul`, `Div`, `Rem`, `Shl`,
`Shr`) lowers, plus the three bitwise operators (`BitAnd`, `BitOr`,
`BitXor`), which cannot trap and so needed no extra scoping to include —
`tests/accept/llvm_arith.ls` exercises all ten, each once, printing a
value chosen so the exact byte proves the answer rather than only
`clang` accepting the module. The six comparisons (`Eq`/`Ne`/`Lt`/`Le`/
`Gt`/`Ge`) lower too, but stay untested end to end: they are this
language's only way to produce a `bool`, and this language has no way to
*observe* a `bool` outside `if`/`match` — control flow this backend still
does not lower (§5's own next gap, not this slice's to close). `&&`/`||`
stay refused outright: `ir.rs`'s own comment says they lower as control
flow rather than as an instruction, for the same reason.

Not built the way this section originally planned: **not** checked
against `benches/guards.c`-style kernels the way
`every_guard_kernel_agrees_across_its_three_modes` checks Cranelift's
three modes against each other. That harness needs a loop to run a
kernel's cases, and a loop is control flow. What replaced it: every
runtime-unknown operand needed for a trap test is a value `putchar`
hands back (an impure call the compile-time folder cannot evaluate,
unlike a bare literal pair — see below), which is enough to prove each
of the seven trapping operators raises `SIGILL` on a real `clang`
without needing a loop to do it in. §3.2's fix and §3.3's per-target
asymmetry are pinned down as tests for x86-64 here; aarch64 is CI-only,
as it was after the first slice.

**A finding this slice's own tests forced, not a design decision made in
advance:** `docs/compile-time.md` §3 folds an operator whose *both*
operands are literals at compile time, and turns one that would overflow
into a refused program (`Rule::ConstantTraps`) rather than a running
one. Writing `checked_add_traps_on_overflow` as `9223372036854775807 + 1`
does not reach this backend at all — it is refused before lowering
finishes, correctly, and for a reason that has nothing to do with either
backend. Every trap test here instead routes one operand through `x`, a
value read back from `probe`, a function that performs `io_write` and so
is never a candidate for constant folding (`docs/purity.md` §2's own
condition). This is not a workaround around the language; it is the
proof that a *genuinely* run-time-unknown overflow reaches the checked-
arithmetic codegen this slice built, rather than one the folder would
have already caught for a different reason.

**A second finding, in the codegen itself:** LLVM's `sdiv`/`srem` are
**undefined**, not trapping, on a zero divisor or on `int::MIN / -1` —
unlike Cranelift's, which trap on both (`docs/defined-behaviour.md`).
The obvious LLVM translation of `Div`/`Rem` — lower straight to `sdiv`/
`srem`, the way `Shl`/`Shr` almost can (masked by `LLVM`'s own semantics
where Cranelift's would need the same explicit check this backend
already writes) — would have been silently wrong on exactly the two
inputs the language exists to make defined. Both checks are explicit,
ahead of the instruction, the same shape `Shl`/`Shr`'s range check
already has: `checked_div_traps_on_division_by_zero` and
`checked_div_traps_on_int_min_over_negative_one` are what prove the
checks actually run, on a real `clang`, rather than merely compile.

**Later slices, each its own PR, not scoped further here:** control flow
(`Stmt::If`/`Stmt::While`/`Stmt::Match` — named explicitly now, where the
first two slices left it implicit, because it is what the comparisons
built here still cannot be observed through, what bounds-checked
indexing needs, and what a loop-based benchmark kernel needs; likely
LLVM's `br`/`phi` rather than this slice's trap-only branching, since a
real `if` merges two live values and this backend has not needed a `phi`
node yet), structs and enums (LLVM's own aggregate types, a more direct
mapping than Cranelift's flattened layout — worth its own measurement
rather than an assumption), arenas and regions (a `Heap`-free allocation
scheme LLVM has no special vocabulary for either, so likely the same
bump-pointer strategy translated rather than redesigned), `Ffi`/`extern
fn` (LLVM's own `declare` is close to a direct match), `Net`. Each is a
`Builtin` or `BinOp` variant this document is not claiming to have
scoped — `ir.rs` has 34 builtins and lists them so the next slice can
pick a subset by reading them rather than guessing at the size of what
is left
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
| ~~The exact `ud2`/`udf` spelling and signal on linux-x86_64~~ | **Measured, this slice's session, on a real linux-x86_64 host**: `call void asm sideeffect "ud2", ""()` assembles to the same two bytes (`0f 0b`) Cranelift's own `ud2` does, and the linked binary exits 132 — `SIGILL` — every time, matching §3.2's aarch64 finding exactly (no `SIGTRAP` substitution, no divergence). One more thing confirmed alongside it: passing `clang -target <triple>` with the same spelling the `.ll` module's own `target triple` line carries silences the `overriding the module target triple` warning §3.1 first saw on darwin — `compile_object_for` always does this, so a program built through this crate never sees it. `aarch64`'s `udf #0xc11f` half of §3.2's fix is still unverified — no aarch64 host in this slice's session either — but this row is otherwise closed |
| Which `clang`/LLVM IR version to target | Textual IR has version-dependent syntax; pinning to whatever `ci.yml`'s two runners ship, rather than a specific LLVM release, is the plan until a real incompatibility forces a choice. This slice's session measured against `clang` 18 on linux-x86_64 only |
| Optimisation level and its effect on trap codegen | `clang -O2` might fold or reorder a checked operation in a way `-O0` would not; `check-cost.md` and `poison.md`'s Cranelift findings do not transfer automatically, and this needs its own measurement once arithmetic lowering exists (§5's second slice) |
| Whether `noalias`/purity attributes get emitted at all | `backend-limits.md` §4's ceiling question — real, and not this document's to answer before the backend can run anything |
