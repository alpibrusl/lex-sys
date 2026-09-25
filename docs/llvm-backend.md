# A second backend, and how it would actually get built

> **Status: first five slices built, and `examples/hello.ls` builds**
> (§5, `lex-sys-codegen-llvm`, `--backend llvm`). §5 originally named
> `hello.ls` as the *first* slice's target; building that slice found the
> claim false — `hello.ls` needed checked arithmetic, bounds-checked
> indexing, string-literal data, and (found along the way) control flow,
> four things across four slices rather than one. The fourth, slices and
> strings, is what closed that loop: `hello.ls` — `ci.yml`'s own smoke
> test — builds and runs through this backend, byte for byte the same as
> through Cranelift. The fifth, structs and enums, is a different kind of
> slice: nothing in this document's original plan asked for it by name
> the way `hello.ls` asked for the first four — it is next because
> `docs/ROADMAP.md`'s own "later slices" list already named it, and
> `tests/accept/enums.ls` turned out to be a ready-made target once it
> landed. `tests/accept/llvm_smoke.ls`, `llvm_arith.ls` and
> `llvm_control.ls` are what the first three slices' own bullet lists
> actually describe; the fourth and fifth needed no new fixture of their
> own, because `hello.ls` and `enums.ls` already were ones. The second
> slice found LLVM's `sdiv`/`srem` are **undefined**, not trapping, on
> the two inputs Cranelift traps — a correction below, not assumed going
> in. The third needed no `phi`: every local here is already memory, so
> an `if`'s two arms and a `while`'s back edge just read whatever was
> last written, the same fact the first slice's `alloca`-per-leaf design
> was for. The fourth reused that fact a third time: a string literal's
> global symbol is already a usable pointer constant in LLVM IR, so
> `Expr::Bytes` needed no instruction at all, only a declaration. The
> fifth reused `trap_if` a third time too, for `match`'s own tag-test
> chain, and needed no `phi` either — the same memory-backed reasoning,
> applied to a chain of branches instead of a diamond or a loop.
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
>
> §7 is a third thing, found later: `run_clang` never passed `clang` an
> optimisation level, so the `mem2reg` promotion §5's third slice called
> "mandatory" never actually ran — every leaf stayed a real stack slot,
> and the backend was measurably *slower* than Cranelift on every kernel
> that builds on both. Fixed by always passing `-O2`; measured again
> after, the same three kernels are **8%–54% faster** than Cranelift, not
> slower, and the seven trap-signal tests still pass unchanged — a
> genuinely false claim, corrected in place rather than left standing.
> §7.4 closed `wrapping_add`/`sub`/`mul` and found LLVM can delete a
> whole no-op loop outright once nothing in it can trap. §7.5 closed
> `region`/`alloc_slice` — the biggest remaining gap — and found the
> first real vectorisation in this document: the wrapping halves of
> `sieve`/`scan` compile to hundreds of SIMD instructions where their
> checked twins, otherwise identical, compile to none. §7.7 closed heap
> boxing and, underneath it, a second gap nothing had tried to lift
> since the first slice: a function could not return more than one
> leaf. `reduce_checked.ls` — `docs/gpu.md`'s own kernel — needed both,
> and confirms the same vectorisation split a fourth time. §7.9 closed
> `getchar` and `s[a..b]`, and found `revcomp.ls`'s own boundary sitting
> one gap further out still: `Place::Field`/`Place::Deref`, named since
> §5 and, until now, never connected to a program that actually needed
> it.

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

**Third slice: control flow — built, and needed no `phi` after all.**
`Stmt::If`/`Stmt::While` both lower, plus the two short-circuit
operators, `And`/`Or`, which `ir.rs`'s own comment already said belonged
here rather than in `binop`'s instruction table. The `phi` node §5's own
text once assumed a real `if` would need never got written: every local
in this backend is already an `alloca` (§5's first-slice design, chosen
so `clang`'s `mem2reg` does the SSA work this crate does not — **at
`-O1` or above**; §7 found and fixed the session where this crate was
still invoking `clang` at its default `-O0`, where `mem2reg` does not
run at all), so the
block after an `if` simply `load`s whatever the taken arm last `store`d,
and a `while`'s loop header is re-entered by its back edge the same way
— fresh loads each time, no value carried in a register across the
edge. `&&`/`||` use the same trick over a one-leaf temporary `alloca`
rather than a `phi` merging two live values. The only real branching
this slice added is the diamond `if_stmt` builds and the loop `br` pair
`while_stmt` builds; `trap_if`'s branches, from the second slice, turn
out to have been this backend's first working example of exactly that
shape, one side of the diamond always `unreachable`.

**What this closes**: comparisons, built and untested since the second
slice, are now exercised end to end — `llvm_control.ls` prints a
different byte depending on each of the six, which is the first time
any of them influenced this backend's output rather than merely
compiling. **What it does not close**: `hello.ls` still refuses at this
point, now on its string literal (`Bytes`) rather than on its `while`
loop — a `--backend llvm` run against it before this slice failed at
the control flow; after, it reaches the greeting itself and fails there
instead, which is this slice's own evidence that the boundary moved
rather than merely that a claim changed. (The fourth slice, next, is
what actually closes it.)

**`Stmt::Match` stays refused**, deliberately not scoped into this
slice: an enum's tag-and-payload layout is a structs-and-enums question
(below), and dispatching on a tag needs a `switch` or a chain of `br`s
this document is not claiming to have designed yet. `Stmt::Region` stays
refused too, for the arenas-and-regions reason already listed below —
neither needed the branching this slice actually built to also need
scoping into it.

**Fourth slice: slices and strings — built, and `hello.ls` builds.**
`Expr::Bytes`, `Expr::Len` and `Expr::Index` all lower, plus
`Builtin::IntOf` (`hello.ls`'s own `int_of(s[n])`, a plain `zext i8` —
necessary plumbing to reach the target, the same reason the first slice
needed `putchar` beyond what its own bullet list first named). Two
things came out cheaper than expected and one came out exactly as
planned:

- **A string literal needed no instruction, only a declaration.**
  `lex-sys-codegen`'s Cranelift path reads a literal's address with
  `global_value`, an instruction Cranelift's SSA builder needs to
  materialise a global into a value. LLVM has no equivalent step: `@sym`
  is already a `ptr` constant wherever one is expected, so
  `bytes_lit` writes the `private unnamed_addr constant` declaration
  into a module-level buffer and hands back the bare symbol name as the
  pointer leaf — nothing is emitted into the function body at all for
  the address half of a literal.
- **Bounds checking is exactly one more `trap_if`.** `s[i]`'s check —
  `icmp uge i64 <index>, <len>` — is textually the same shape
  `checked_shift`'s range check already is, reusing `trap_if` rather
  than adding a new branching primitive. The one design decision this
  slice made was plain integer-array syntax (`[i8 72, i8 105, ...]`)
  over LLVM's `c"..."` string-constant shorthand, to avoid writing (and
  getting wrong) a second escaping rule for characters the shorthand
  does not accept directly.
- **`docs/compile-time.md`'s constant folder does not reach `Index` at
  all**, unlike the second slice's own finding about `BinOp`. A literal
  out-of-range index (`"abc"[5]`) passes `lex-sys check` and only traps
  at run time, which is what let this slice's own bounds-check tests
  (`indexing_past_a_slice_traps_with_sigill`,
  `indexing_before_a_slice_traps_with_sigill`) use a literal index
  directly, with none of the second slice's `probe`-routed workaround.

`Place::Element` (`s[i] = e`) lowers too, by the same `element_address`
the read side uses, so indexed writes are not a silent asymmetric gap
next to indexed reads. `Place::Field`/`Place::Deref` stay refused —
both need struct/box layout, structs-and-enums' question below.

**Fifth slice: structs and enums — built.** `Expr::Struct` needed
almost nothing new: a struct value is positional already (declaration
order, not source order), so its leaves are just every field's leaves
concatenated — the same shape `Expr::Tuple` would be, and this backend
already reads a struct's fields this way through `Expr::Field`, one
slice's worth of reading built before any construction was.

`Expr::Enum` and `Stmt::Match` are where the real work was, and both
matched `lex-sys-codegen`'s own layout exactly rather than inventing a
narrower one: an enum's leaves are a tag (`i64`) followed by **every**
variant's payload leaves, not only the constructed one — wasteful and
deliberately so, because overlaying payloads is a layout decision this
milestone makes none of. `variant_layout` computes where one variant's
slice of that whole starts, shared by both directions: `enum_lit`
writes the tag and the constructed variant's values, zero-filling
every other variant's leaves (a value that is not this variant is not
readable without matching on the tag first, so what is actually there
is unobservable); `bind_payload` reads the same offsets back out when a
`match` arm binds them. `match_stmt` lowers to a chain of tag tests —
`icmp eq` against each arm's variant, in order, falling through on a
miss — matching `lex-sys-codegen`'s own comment that a jump table would
be faster and is the obvious later move for both backends alike, not
just this one.

**One real subtlety, not obvious until measured against `if_stmt`'s
own shape:** a `match` whose every tested arm returns still needs a
`merge` block, unlike `if_stmt`, which omits one in the equivalent
case. The reason is exhaustiveness's two different sources. `if`/`else`
are syntactically exhaustive — there is always a real `else`, even an
empty one — so §5's third slice could simply not emit a fall-through
edge when both sides terminate. A `match` over enum variants is only
exhaustive because the *checker* proved every variant is covered, which
a chain of `icmp`s does not know on its own: with no wildcard arm, the
chain's final `next` block falls through what the value's type makes
impossible but the IR does not. That block still needs a legal
terminator, so `match_stmt` always emits `merge` and gives it a default
return exactly when nothing valid reaches it — the same "unreachable
but must still be well-formed" reasoning `emit_default_return` (factored
out of `emit`'s own function-level fall-through in this slice) already
had one job for.

**Deliberately not in this slice:** matching *through* a reference.
`docs/reading-references.md`'s address-only binding mode — where a
matched payload is a pointer into the referent rather than a copy of
it, and costs nothing because nothing is loaded — has no counterpart
here; `by_reference` scrutinees are refused outright. Getting that
right needs the same pointer-into-a-referent arithmetic
`Place::Field`/`Place::Deref` need, which is also still refused, for
the same reason: none of it was needed to make `tests/accept/enums.ls`
build, and `enums.ls`'s own `match` binds an *owned* `Point` (`Shape::
At(p, r) => p.x + p.y + r`), read back through the `Expr::Field` this
backend already had.

**Later slices, each its own PR, not scoped further here:** matching
through a reference and `Place::Field`/`Place::Deref` (both need
pointer arithmetic into a referent — the address-only counterpart to
what this slice built by value), arenas and regions (a `Heap`-free
allocation scheme LLVM has no special vocabulary for either, so likely
the same bump-pointer strategy translated rather than redesigned;
`Stmt::Region` waits on this), `Ffi`/`extern fn` (LLVM's own `declare`
is close to a direct match),
`Net`. Each is a `Builtin` or `BinOp` variant this document is not
claiming to have scoped — `ir.rs` has 34 builtins and lists them so the
next slice can pick a subset by reading them rather than guessing at the
size of what is left (`crates/lex-sys-ir/src/builtin.rs`).

**Partially answered, later, in §7: is it faster.** This section still
holds as written for the session that wrote it — five slices in, nothing
here had measured a ratio, and `backend-limits.md` §4's warning against
overclaiming what LLVM's vocabulary buys stands regardless of the
number. §7 is where a first, narrow measurement was actually taken, once
three real kernels built on both backends, and it found a bug in this
crate before it found a ratio worth reporting.

---

## 6. Open

| Question | Why it waits |
|---|---|
| ~~The exact `ud2`/`udf` spelling and signal on linux-x86_64~~ | **Measured, this slice's session, on a real linux-x86_64 host**: `call void asm sideeffect "ud2", ""()` assembles to the same two bytes (`0f 0b`) Cranelift's own `ud2` does, and the linked binary exits 132 — `SIGILL` — every time, matching §3.2's aarch64 finding exactly (no `SIGTRAP` substitution, no divergence). One more thing confirmed alongside it: passing `clang -target <triple>` with the same spelling the `.ll` module's own `target triple` line carries silences the `overriding the module target triple` warning §3.1 first saw on darwin — `compile_object_for` always does this, so a program built through this crate never sees it. `aarch64`'s `udf #0xc11f` half of §3.2's fix is still unverified — no aarch64 host in this slice's session either — but this row is otherwise closed |
| Which `clang`/LLVM IR version to target | Textual IR has version-dependent syntax; pinning to whatever `ci.yml`'s two runners ship, rather than a specific LLVM release, is the plan until a real incompatibility forces a choice. This slice's session measured against `clang` 18 on linux-x86_64 only |
| ~~Optimisation level and its effect on trap codegen~~ | **Measured, §7**: `clang -O2` is now what `run_clang` always passes (it has to be, for `mem2reg` to run at all — §7). All sixteen `lex-sys-codegen-llvm` unit tests, including all seven trap-signal tests, pass unchanged under it: `-O2` does not fold, reorder, or eliminate a checked operation's trap on any kernel this backend can build today |
| Whether `noalias`/purity attributes get emitted at all | `backend-limits.md` §4's ceiling question — real, and not this document's to answer before the backend can run anything |

---

## 7. `-O2`, a first measurement, and what still blocks the rest of the suite

### 7.1 The bug: `clang` was never asked to optimise anything

§5's third slice justified the memory-backed design on `clang`'s
"mandatory `mem2reg`". That is true of `clang`'s standard pipeline, and
false of what this crate was actually invoking: `run_clang` (`lib.rs`)
called `clang -c -target <triple> <file>.ll`, no `-O` flag at all, and
`mem2reg` is not part of `-O0`. Confirmed directly — a minimal `.ll`
identical in shape to `sum_checked.ls`'s inner loop (`alloca`, `store`,
loop back-edge reading with `load`) compiles at the default optimisation
level to nine real stack `load`/`store` instructions per iteration; at
`-O2` the loop promotes to registers and, for a closed-form case like
this toy one, disappears into a handful of instructions entirely. Every
one of this backend's own tests still passed, because none of them time
anything — `docs/llvm-backend.md`'s own §5 said as much when it deferred
the speed question, but the reason it stayed deferred was this bug, not
only that no kernel built yet.

**Fixed**: `run_clang` now always passes `-O2`. This is not tunable per
build — the backend has no `--opt-level` flag, and `docs/llvm-backend.md`
is not proposing one; an "unoptimised LLVM backend" mode is not a
configuration this project has a use for, since the entire reason to
reach for `clang` over Cranelift is the optimisation pipeline §3.1
already established `clang` brings for free. All sixteen
`lex-sys-codegen-llvm` unit tests pass under `-O2`, including the seven
that check a checked operation's trap raises the exact signal Cranelift
raises (`the_two_backends_agree_on_*` in `crates/lex-sys/tests/
conformance/backends.rs` also re-checks this at the CLI level) — `-O2`
changes the code the trap sits inside, not whether or how it fires.

### 7.2 A first backend-vs-backend measurement

Three of `benches/`'s kernels build on both backends today, unchanged:
`sum_checked.ls` (tight checked arithmetic, no memory traffic — the
"worst case" for the overflow check, `sum_checked.ls`'s own header),
`fib_checked.ls` (recursion; the cost is calls, not arithmetic) and
`benches/three/mandelbrot.ls` (Q16.16 fixed-point compute, the kernel
`docs/against-c-and-rust.md` already runs against C and Rust). All three
are covered by a new differential test each
(`the_two_backends_agree_on_sum_checked`,
`..._on_fib_checked`, `..._on_mandelbrot` in `backends.rs`) and by
`scripts/backend_compare.py`, which is `scripts/bench.py`'s own
interleaved-minimum method applied to backend choice instead of
source-program choice.

Measured on this session's linux-x86_64 host, `--rounds 40`, minimum of
each interleaved half (the run is noisy — a shared, non-dedicated
host — so the **direction and rough size** of the gap is the finding,
not the exact percentage):

```
program        cranelift        llvm      llvm/cranelift
sum_checked      0.227s       0.105s            -54%
fib_checked      0.010s       0.009s            -11%
mandelbrot       0.148s       0.092s            -38%
```

LLVM is faster on all three, once `-O2` actually runs — consistent with
`backend-limits.md`'s and `check-cost.md`/`gpu.md`'s original hypothesis
(a missing vectoriser and missing per-target instruction selection are
what the four-line Cranelift predicate they identify is blocked on), and
the opposite of what §7.1's bug alone measured (LLVM 14%–58% *slower*
across the same three kernels before the fix). `fib_checked`'s narrower
gap is the expected shape: call/return overhead, not arithmetic, is that
kernel's critical path, and a better arithmetic pipeline has less of the
run to speed up. **This is not `backend-limits.md`'s full claim measured
yet** — none of these three kernels exercises the memory-bound or
SIMD-shaped cases (`sieve`, `scan`, `reduce`, the Benchmarks Game
programs) that motivated the backend in the first place; §7.3 is why.

**Against C, not just against Cranelift** — `scripts/backend_compare.py
--with-c` adds a third, three-way interleaved leg for `mandelbrot.ls`
against `mandelbrot.c`, the exact kernel `docs/against-c-and-rust.md`'s
1.6×/1.69× headline came from:

```
mandelbrot, three-way interleaved against clang -O2 (20 rounds):
  cranelift  0.1504s  (1.633x clang)
  llvm       0.0887s  (0.964x clang)
  clang      0.0921s  (1.000x clang)
```

`docs/against-c-and-rust.md` §2 stated a falsifier for exactly this
number: *"if an LLVM backend lands and the gap stays at 1.6×, the claim
was wrong."* It did not stay — lex-sys through `--backend llvm` is
**indistinguishable from C** on this kernel (0.96×–1.00× across repeated
runs), while `--backend cranelift` reproduces the original 1.6×–1.8×
almost exactly. `docs/against-c-and-rust.md` §2 now carries this
correction in place, next to the claim it falsifies.

**Not vectorisation — checked, not assumed.** `gpu.md`'s own methodology
warns against reading SIMD off the clock; `objdump -d` on both
`sum_checked_llvm` and `mandelbrot_llvm`'s `.text` finds **zero**
`%xmm`/`%ymm`/`%zmm` instructions, at `-O2`, on either. What actually
changed against the pre-fix disassembly: every leaf that was a real
`-0x10(%rsp)` load/store is now a register (`mem2reg`, finally running),
and `sum_checked`'s loop is partially unrolled ×2 with the same
`add`/`jo`/`sub`/`jo` sequence Cranelift already emits, just scheduled
better and with no memory traffic between iterations. So `overflow-
cost.md`/`check-cost.md`'s established finding — an observable trap is
not reassociable, so a checked loop does not vectorise — **holds here
too**, confirmed on a real backend rather than only inferred from
Cranelift's absence of one: this is `docs/ROADMAP.md`'s "What is next"
row's own open question, and the answer is **no, still scalar**, not
"a vectoriser fixed it." The whole measured gain in §7.2 is register
allocation and instruction selection catching this backend up to what a
competent scalar compiler already does — real, and worth having, and a
different, smaller claim than "LLVM vectorises checked arithmetic."

### 7.3 What still blocks the rest of `benches/` from being comparable

Not a request for the LLVM backend to reach Cranelift's *feature* set in
the abstract — a request for exactly what would make the rest of the
existing benchmark suite buildable on both backends, so the comparison
`backend-limits.md` actually wants (memory-bound, checked-vs-wrapping,
SIMD-shaped) can be taken. Found by trying to build every file under
`benches/` with `--backend llvm` and reading the refusal, not by
inspecting `emit.rs` and guessing:

| Gap | Blocks | Where it already shows up in this document |
|---|---|---|
| ~~`region`/`alloc_slice` (arena allocation, `Stmt::Region`)~~ | **Closed, §7.5**: `sieve_*.ls`, `scan_*.ls`, `benches/three/sieve.ls` now build (`fasta.ls`/`revcomp.ls` still refuse, on `Type::Float` and `getchar` respectively — their own rows below) | §5's "later slices" list, already named |
| ~~`wrapping_add`/`wrapping_sub`/`wrapping_mul`~~ | **Closed, §7.4**: every `_wrapping.ls` half of a `benches/` pair and `benches/three/purity.ls` now build on `--backend llvm` | Implicit in §5's "every `Builtin` beyond `PutChar`/`Split`/`Release`/`Narrow`/`IntOf`"; not previously named on its own |
| Bare `Expr::Alloc` (single-value arena allocation) | `tests/accept/arena_roundtrip.ls` — this backend's own "outside the boundary" fixture, not a `benches/` program | Not previously named on its own; distinguished from `alloc_slice` only once §7.5 closed the latter |
| ~~`box_slice`/`Contents`/`unbox_slice`~~ | **Closed, §7.7**: `reduce_*.ls`, every `benches/layout/*.ls` file now build (bare `box`/`unbox`, a single-value box, stay refused — nothing in `benches/` asks for one) | Same bucket as above; not previously named on its own |
| `arg_count` (and argument reading generally) | `benches/game/binarytrees.ls`, `benches/game/fannkuch.ls` | Same bucket |
| `Type::Float` and float arithmetic | `benches/game/spectral.ls`, `benches/game/fasta.ls` (two `alloc_slice` fills) | `emit.rs`'s own `LKind` doc comment already says floats are refused; not previously named as a *benchmark*-blocking gap |
| ~~`getchar`/`io_read`~~ | **Closed, §7.9** (`tests/accept/stdin_roundtrip.ls` is the fixture; `revcomp.ls` itself needs `Place::Field`/`Place::Deref` too, found the same slice — its own row above, not closed) | Found trying to build `revcomp.ls` once §7.5 closed `region`; not previously named |

Ordered by what it would unblock, as each closed: **`wrapping_*` first**
(§7.4) — smallest of the remaining gaps, and it made the overflow-
check's own cost (`docs/overflow-cost.md`'s question) measurable on
this backend for the first time. **Arenas next** (§7.5) — the biggest
single gap by program count, and building against the real targets
found two smaller gaps (`byte_of`, `Expr::Not`) sitting in front of it
that no inspection of `emit.rs` alone would have named. **Heap boxing
after that** (§7.7) — the next-biggest pocket, and building
`reduce_checked.ls` against it found a second, unrelated gap
underneath: multi-leaf function returns, closed the same session.
**`getchar` next** (§7.9) — smallest by design, and building `revcomp.ls`
against it found `s[a..b]` in front, closed alongside it, and
`Place::Field`/`Place::Deref` behind both, not closed. What is left —
bare `alloc`, `Place::Field`/`Place::Deref`, `arg_count`, float — is
four gaps, one of them (`Place::Field`/`Place::Deref`) larger than a
single builtin or bounds check (§7.10 has the current, complete list).

| Bench | |
|---|---|
| `scripts/backend_compare.py` | Interleaved cranelift-vs-llvm timing on the kernels that build on both, today eleven; `--with-c` adds a three-way leg for `mandelbrot.ls` against `mandelbrot.c` |
| `crates/lex-sys/tests/conformance/backends.rs` | `the_two_backends_agree_on_{sum_checked,fib_checked,mandelbrot,sum_wrapping,fib_wrapping,purity,sieve_checked,sieve_wrapping,scan_checked,scan_wrapping,the_three_language_sieve,reduce_checked,reduce_wrapping,layout_aos,layout_soa,layout_ints,layout_rgb,stdin_roundtrip}` — not a timing gate, for the reason `every_benchmark_pair_agrees` gives |
| `crates/lex-sys-codegen-llvm/src/tests.rs` | `{subslicing_past_the_end,an_inverted_subslice}_traps_with_sigill` — the two ways `s[a..b]` can be wrong, checked by signal the same way every other trap here is |
| `crates/lex-sys-codegen-llvm/src/tests.rs` | `allocating_past_an_arenas_chunk_traps_with_sigill` — the arena-exhaustion trap, checked by signal the same way every other trap here is |

### 7.4 `wrapping_add`/`sub`/`mul`, closed — and what removing a trap buys an optimiser

§7.3's smallest-first ordering: `wrapping_add`, `wrapping_sub`,
`wrapping_mul` lower to LLVM's own `add`/`sub`/`mul`, no `nsw`/`nuw`
requested — already two's-complement wraparound, the direct counterpart
of `lex-sys-codegen`'s plain `iadd`/`isub`/`imul` (`crates/lex-sys-
codegen/src/body/expr.rs`). No overflow check, no new type, no new
`Place`: three match arms and a four-line helper (`emit.rs`'s `wrapping`,
built on the `plain` helper `BitAnd`/`BitOr`/`BitXor` already used).

Every checked-vs-wrapping pair in `benches/` that does not also need
`region`/`box_slice` now builds on `--backend llvm`: `sum_wrapping.ls`,
`fib_wrapping.ls`, and `benches/three/purity.ls` besides (which needed
this and nothing else). Three new differential tests
(`the_two_backends_agree_on_{sum_wrapping,fib_wrapping,purity}`) join
§7.2's three.

**The interesting number is not the ratio — it's why one measurement
looked broken until it was checked.** `scripts/backend_compare.py`
first reported `sum_wrapping.ls` at **−99.1%** against Cranelift
(0.0012s against 0.1286s), which is not "faster," it is a different
program running. `objdump` on the object confirms it: `lexs_run`
compiles to `xor %eax,%eax; ret` — two instructions, no loop at all.
`sum_wrapping.ls`'s inner loop is `total = total + i; total = total -
i; i = i + 1`, which is mathematically a no-op on `total` regardless of
`i`'s value, and — unlike `sum_checked.ls`'s identical shape — **nothing
in the wrapping version can trap**, so there is no observable effect
left for the optimiser to have to preserve by actually running the two
hundred million iterations. LLVM proves this and deletes the loop.
Cranelift does not perform this optimisation at all (`overflow-cost.md`/
`check-cost.md` already established why the *checked* twin can't be
deleted this way — an observable trap is not reassociable, and deleting
a loop that might trap would delete the trap too); §7.2's disassembly
check already found LLVM does not *vectorise* a checked loop for the
same reason. This is the same fact from the other side: remove the
thing that makes a loop's effects observable, and an optimiser that has
one can prove the loop is worth nothing and skip it entirely, which no
amount of instruction selection would have bought on its own.

`fib_wrapping.ls` is the number worth trusting instead — recursion with
data-dependent branching has no such algebraic identity to collapse, and
`objdump` confirms real, unrolled code (21 instructions in `lexs_fib`,
not two). Measured, **−23.3%** against Cranelift, in the same range
§7.2's other three kernels landed in.

This also answers a question `docs/overflow-cost.md` could only ask
of Cranelift before: what the checked-vs-wrapping guarantee costs under
LLVM. `sum_checked.ls` cannot collapse the way `sum_wrapping.ls` did —
its `+`/`-` can trap, so the loop's iteration count is observable even
though the arithmetic result is not — and it still runs in **0.10s**
against Cranelift's **0.22s** (§7.2). The overflow check's cost on this
backend is not yet its own document's worth of measurement; it is a
byproduct worth naming here: on this one kernel, the gap between checked
and wrapping is far larger under LLVM (a real loop vs. no loop at all)
than under Cranelift (`overflow-cost.md`'s own **+40.5%** on the same
kernel shape) — because LLVM had a bigger optimisation to lose.

### 7.5 `region`/`alloc_slice`, closed — and a vectoriser firing for the first time

§7.3's second-named gap, and the bigger of the two remaining: `Stmt::
Region`/`Expr::AllocSlice` need real arena allocation, which this
backend had none of. Built the same shape `lex-sys-codegen`'s own
`body/memory.rs` already has — one `malloc` in, one `free` out, and a
bump pointer that only ever moves forward within the chunk — kept in two
`ptr`-typed `alloca` cells rather than in an SSA value, this backend's
own idiom for anything that needs to vary rather than an
`lex-sys-codegen` `Variable`. `bump`'s bounds check is textually the
same two-comparison shape (`next` past `end`, or `next` wrapped below
`at`) `lex-sys-codegen`'s own `bump` makes, translated into `ptr`
arithmetic throughout (`getelementptr`/`icmp` both work directly on
`ptr` in LLVM IR, so nothing here needs `ptrtoint`). `ARENA_CHUNK` is
the same 64 KiB constant, unmodified from `lex-sys-codegen`'s
`abi::ARENA_CHUNK` — `sieve_checked.ls`'s own header sizes its
allocation against this exact number, so a mismatched chunk would trap
where the Cranelift build does not, or the reverse.

Building `sieve`/`scan` against this found two smaller gaps actually
sitting in front of them, not named until tried: `byte_of` (narrow-or-
trap, `docs/strings.md` §2 — one unsigned comparison and a `trunc`, the
same shape `element_address`'s own bounds check already has) and
`Expr::Not` (`!b` — a `bool` leaf is 0 or 1, so flipping the low bit is
the negation, one instruction, never trapping). Neither is its own row
in §7.3's table; both were found by building the actual target and
reading the refusal, the same method §7.3 itself used.

Every program behind only these four gaps now builds on `--backend
llvm`: `sieve_checked.ls`, `sieve_wrapping.ls`, `scan_checked.ls`,
`scan_wrapping.ls`, and `benches/three/sieve.ls` — five differential
tests join the nine already in `backends.rs`, checked byte-for-byte
(or exit-code-for-exit-code) against Cranelift, plus a new crate-level
trap test: `allocating_past_an_arenas_chunk_traps_with_sigill`,
requesting 9000 `int`s (72000 bytes) from one arena and checking the
same `SIGILL` §3.2 already established for every other trap here.
`fasta.ls`/`revcomp.ls` — §7.3's other two `region`-blocked programs —
still refuse: `fasta.ls` on a `float`-typed `alloc_slice` fill, and
`revcomp.ls` on `getchar` (`io_read`), each its own already-named gap.

**Measured, `--rounds 25`, minimum of each interleaved half:**

```
program           cranelift        llvm      llvm/cranelift
sieve_checked        0.173s       0.122s            -29%
sieve_wrapping       0.144s       0.061s            -58%
scan_checked         0.192s       0.061s            -68%
scan_wrapping        0.192s       0.040s            -79%
```

These are the first memory-bound, bounds-checked kernels measured
backend-vs-backend — every program in §7.2/§7.4 was either pure
arithmetic or recursion. LLVM is faster on all four, in the same
direction as every kernel so far.

**And, checked with `objdump` rather than assumed: the wrapping halves
of these two pairs are the first kernels in this document where LLVM
actually vectorises.** `scan_wrapping.ls`'s object has **409**
`%xmm`/`%ymm`/`%zmm` instructions; `sieve_wrapping.ls`'s has **22**.
`scan_checked.ls` and `sieve_checked.ls` — otherwise identical source,
diffed to confirm the only change is `+`/`-` becoming `wrapping_add`/
`wrapping_sub` at every site, including the loop's own induction
variable — have **zero**, matching §7.2's and §7.4's finding on
`sum_checked.ls`/`mandelbrot.ls` exactly. `check-cost.md`'s "an
observable trap is not reassociable" is not a claim about arithmetic
kernels specifically; this is the first direct evidence it holds for a
bounds-checked memory scan too, on a real backend, both directions of
the comparison in the same controlled pair. `sieve_wrapping.ls`'s
smaller SIMD count against `scan_wrapping.ls`'s is not measured further
here — its inner loop's stride (`m = m + p`) is data-dependent, not
unit, which is a harder shape to vectorise regardless of trapping, and
untangling how much of the gap is that versus something else is its own
question this document is not answering today.

### 7.7 Heap boxing, closed — and a gap that turned out to be two

§7.6's largest remaining pocket: `box_slice`/`contents`/`unbox_slice`
(`Expr::BoxedSlice`/`Expr::Contents`/`Expr::UnboxedSlice`), needed by
`reduce_*.ls` and every `benches/layout/*.ls` file. Built the same
shape `alloc_slice` already is — `boxed_slice` shares `alloc_slice`'s
own `slice_bytes` helper (factored out once there were two callers) for
the checked-multiply sizing, and reaches for `malloc`/a null trap
instead of `bump`/an arena chunk; `contents` and `unbox_slice` are each
one load and one `free` respectively, matching `lex-sys-codegen`'s own
`body/memory.rs` exactly. Bare `Boxed`/`Unboxed` (a single-value box,
not a slice) stay refused — nothing in `benches/` asks for one.

**Building `reduce_checked.ls` against this found a second, unrelated
gap sitting underneath it: multi-leaf function returns.** `fill`, the
helper that builds `reduce_checked.ls`'s array, returns `Box[[int]]` —
two leaves, a pointer and a length (`docs/boxed-slices.md` §2) — and
`emit`/`call` had refused any function return past one leaf since the
first slice, a restriction nothing had tried to lift because nothing
had needed to yet. Closed the same way LLVM's own multi-result
intrinsics already read back in this file: a multi-leaf return packs
into one anonymous struct (`{ptr, i64}` for a boxed slice, via
`insertvalue`, mirroring how `checked_arith` already unpacks `{i64,
i1}` out of `@llvm.sadd.with.overflow.i64` with `extractvalue`), and a
call site unpacks the same way. Not scoped to two leaves specifically —
`struct_ty`/`pack_struct` take any number of kinds, so a three-field
struct return works the same way, untested only because nothing in
`benches/` needs one yet.

Every program behind only these gaps now builds on `--backend llvm`:
`reduce_checked.ls`, `reduce_wrapping.ls`, and all four `benches/
layout/*.ls` files (`aos.ls`, `soa.ls`, `ints.ls`, `rgb.ls`) — six
differential tests join the eleven already in `backends.rs`, all
checked byte-for-byte (or exit-code-for-exit-code) against Cranelift;
the existing seventeen `lex-sys-codegen-llvm` unit tests, exercising
every earlier slice's own return path, still pass unchanged.

**Measured, `--rounds 20`, minimum of each interleaved half:**

```
program            cranelift        llvm      llvm/cranelift
reduce_checked        0.248s       0.096s            -61%
reduce_wrapping       0.100s       0.068s            -32%
```

`reduce_checked.ls` is `docs/gpu.md`'s own kernel — "the shape a GPU
runs," its own header says — and the first time it has been measured
llvm-vs-cranelift rather than only cranelift-vs-C/Rust. **Checked with
`objdump`, once more before trusting it**: `reduce_checked.ls` has
**zero** SIMD instructions and `reduce_wrapping.ls` has **54**, the
same pattern §7.5 found in `sieve`/`scan` and §7.2/§7.4 found in
`sum`/`mandelbrot` — an observable trap blocks vectorisation, on a
fourth and different kind of kernel now (a boxed-slice reduction,
after arithmetic, recursion, and a bounds-checked memory scan).

The four layout kernels were not separately timed here — `scripts/
backend_compare.py` stays scoped to `benches/`'s own checked/wrapping
pairs, which `benches/layout/` is not shaped as (each file is its own
point, not a pair), consistent with `scripts/three.py`/`scripts/
game.py` already existing as the tools for differently-shaped suites
rather than folding every kind of benchmark into one script.

### 7.8 What still blocks the rest of `benches/`, updated

`box_slice`/`contents`/`unbox_slice` and multi-leaf returns move out of
§7.6's table. What is left: bare `Expr::Alloc` (a single-value arena
allocation — `tests/accept/arena_roundtrip.ls`, not a `benches/`
program but this backend's own "outside the boundary" fixture),
`arg_count` (`benches/game/{binarytrees,fannkuch}.ls`), `Type::Float`
(`benches/game/spectral.ls`, and `fasta.ls`'s two `float`-filled
`alloc_slice` calls), and `getchar`/`io_read` (`benches/game/
revcomp.ls`). Four gaps, each its own single pocket, none bundled with
anything else in `benches/` — the same shape §7.6 described, one
row shorter.

### 7.9 `getchar` and `s[a..b]`, closed — and `revcomp.ls`'s boundary moves past both

`getchar`'s own row: the mirror of `putchar`, sign-extended the same
way, no argument to erase or narrow since `io: &!i Io` is already zero
leaves. Trying it against `revcomp.ls` immediately found a second gap
sitting in front, the same way `sieve`/`scan` found `byte_of`/`Expr::
Not` in §7.5: `s[a..b]` (`Expr::Subslice`), used by the buffer helpers
`revcomp.ls` calls through `std.buffer`. Built the same shape
`element_address` already is — the same two bounds checks
`lex-sys-codegen`'s own `subslice` makes (past the end, or inverted),
then a pointer-and-length pair rather than one element. Two new
crate-level trap tests (`subslicing_past_the_end_traps_with_sigill`,
`an_inverted_subslice_traps_with_sigill`) join the existing bounds-check
pair.

Neither gap had a `benches/` fixture of its own to build against, so
`tests/accept/stdin_roundtrip.ls` — the repository's own first `//~
STDIN` fixture — is what closes the loop this time: a `getchar`/
`putchar` echo loop, piped `"hello\nworld\n"` and checked byte-for-byte
against Cranelift's output, which needed its own comparison in
`backends.rs` rather than reusing `assert_backends_agree` (nothing
built here before piped a program's stdin).

**`revcomp.ls` itself still refuses, and the boundary moved a second
time in the same session — worth stating exactly, not left as "still
blocked on `getchar`" now that it is not.** Past `getchar` and `s[a..b]`,
it reaches `std.buffer`'s `Buffer.clear`, which writes a field through a
reference (`b: &!b Buffer`) — `Place::Field`/`Place::Deref`, already
named in §5's fourth slice as needing "pointer arithmetic into a
referent this backend has not built" and never closed since. This is a
materially bigger gap than either of this slice's two — general
struct-field writes through any reference, not one more builtin or one
more bounds check — and is not scoped here. `revcomp.ls` is removed
from the gap table below on that basis: it is no longer blocked by
`getchar`, but it is still blocked, by something this document already
knew about and had not yet connected to this specific program.

### 7.10 What still blocks the rest of `benches/`, updated again

`getchar`/`io_read` moves out of §7.8's table, closed. `revcomp.ls`
stays off the "now builds" list, moved instead under `Place::Field`/
`Place::Deref` below, where it belongs now that `getchar` is not what
stops it:

| Gap | Blocks | Where it already shows up in this document |
|---|---|---|
| Bare `Expr::Alloc`/`Expr::Boxed`/`Expr::Unboxed` (single-value allocation, arena or heap) | `tests/accept/arena_roundtrip.ls` — this backend's own boundary fixture, no `benches/` program | §7.6, §7.8 |
| `Place::Field`/`Place::Deref` (writing through a reference) | `benches/game/revcomp.ls` (`std.buffer`'s `Buffer.clear`) | §5's fourth slice, named and deferred there; not connected to a `benches/` program until §7.9 |
| `arg_count` | `benches/game/{binarytrees,fannkuch}.ls` | §7.3 |
| `Type::Float` | `benches/game/spectral.ls`, `fasta.ls`'s two `float`-filled `alloc_slice` calls | §7.3 |

Four gaps, one of them (`Place::Field`/`Place::Deref`) larger than
anything else left in this table — every other row is a builtin, a
bounds check, or a single-value allocation shape; this one is general
pointer arithmetic into an arbitrary referent, the same size of gap
matching through a reference has been since §5.
