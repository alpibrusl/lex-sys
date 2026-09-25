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
| ~~`Place::Field`/`Place::Deref` (writing through a reference)~~ | **Closed, §7.11**: `benches/game/revcomp.ls` now builds | §5's fourth slice, named and deferred there; not connected to a `benches/` program until §7.9 |
| `arg_count` | `benches/game/{binarytrees,fannkuch}.ls` | §7.3 |
| `Type::Float` | `benches/game/spectral.ls`, `fasta.ls`'s two `float`-filled `alloc_slice` calls | §7.3 |

Four gaps, one of them (`Place::Field`/`Place::Deref`) larger than
anything else left in this table — every other row is a builtin, a
bounds check, or a single-value allocation shape; this one is general
pointer arithmetic into an arbitrary referent, the same size of gap
matching through a reference has been since §5.

### 7.11 `Place::Field`/`Place::Deref`, closed — and everything sitting behind the same name

The gap §7.10's table called one row was, on contact, four IR nodes on
the write side and five on the read side, none of them named until this
slice actually built against `revcomp.ls`:

- **Writes**: `Place::Deref` (`*r = e`, the whole referent replaced) and
  `Place::Field` (`r.x = e`, one field through a reference) — the two
  named in §5's fourth slice.
- **Reads**, never previously named on their own because nothing had
  tried building a program that used them through this backend:
  `Expr::Deref` (`*r`), `Expr::FieldRef` (`r.x`, a field's value through
  a reference), `Expr::FieldAddr` (`r.x` where the field is `res`, a
  reference *to* the field rather than a copy — `docs/reading-
  references.md` §2.0), and their tuple-shaped counterparts, `Expr::
  TupleFieldRef`/`Expr::TupleFieldAddr`.
- **`Expr::Tuple`/`Expr::TupleField`** — building an owned tuple and
  reading one of its components — were also never built, found only
  because `tests/accept/tuple_roundtrip.ls` was reached for as a test
  fixture and refused before it could serve as one.

All nine share one new helper, `field_offset` (and its tuple-shaped
twin, `tuple_field_offset`): the byte offset of one field among a
struct's (or tuple's) leaves, computed once and used by every one of
`Expr::Field`/`FieldRef`/`FieldAddr` and `Place::Field` alike — the
same arithmetic `lex-sys-codegen`'s own `write` doc comment already
named ("the same field arithmetic as reading one, running the other
way"), collected into one function here rather than repeated five
times. Every address is `ptr` arithmetic throughout
(`getelementptr`/`load`/`store`, no `ptrtoint`), the same idiom
§7.5's arena code and §7.9's `element_address` already established.

**Building `revcomp.ls` against this found one more gap underneath,
the same way every slice since §7.3 has: `write_bytes`/`write_err`**
(`docs/bulk-io.md` §3), needed once `std.buffer`'s own field writes
stopped being the reason it refused. `fwrite` through `stdout`/
`stderr` — `FILE *` *variables* in libc, so the symbol is the address
of the pointer and the stream is one load away, and the symbol differs
by platform (`__stdoutp`/`__stderrp` on Darwin, `stdout`/`stderr`
elsewhere) exactly the way `lex-sys-codegen`'s own `emit.rs` already
resolves it.

**`revcomp.ls` itself now builds, runs, and matches the Benchmarks
Game's own published output on `--backend llvm`** — checked against
`benches/game/revcomp-1000.txt`, the same reference
`benchmarks.rs`'s `fasta_and_reverse_complement_print_the_published_
answer` already checks `--backend cranelift` against. Two more real
fixtures needed nothing but what this slice built:
`tests/accept/deref_roundtrip.ls` (the `int`/`Point` reference
round-trip named above) and `tests/accept/tuple_roundtrip.ls` (tuples
through a reference). All three are new differential tests in
`backends.rs`; `deref_roundtrip.ls` also gets a crate-level test,
matching every earlier slice's own fixture-plus-differential pair.

**Measured** (piped a 1,000,000-record FASTA input generated by
`--backend cranelift`'s own `fasta.ls`, since `fasta.ls` itself still
needs `Type::Float` to build on `--backend llvm`): `revcomp.ls` is
**42%–46% faster** on `--backend llvm`, in the same range every other
kernel in this document has landed in. `objdump` finds **23** SIMD
instructions in the `--backend llvm` object — real, but far short of
`scan_wrapping.ls`'s 409 (§7.5): `revcomp.ls`'s read side is `getchar`,
one byte at a time (`bulk-io.md` §3.3 — there is no bulk read to reach
for), which `benchmarks-game.md` §7 already found is what keeps this
exact program's Cranelift-vs-C ratio inside the ordinary range rather
than at either extreme; the same ceiling applies here.

**Still refused, and not to be confused with what this slice built:
*matching* through a reference** (`Stmt::Match`'s own `by_reference`
flag) is a different feature from reading or writing a field through
one — it needs a match arm to bind a *pointer into the scrutinee*
rather than a copy of it (`docs/reading-references.md`'s address-only
binding mode), which is its own, still-unbuilt address arithmetic over
an enum's variant layout. `match_stmt` still refuses it outright, in
the same words §5 always has.

### 7.12 What still blocks the rest of `benches/`, updated again

`Place::Field`/`Place::Deref` and the read-side cluster behind it move
out of §7.10's table, closed. Matching through a reference is added as
its own row for the first time — it was always refused, but §7.10 and
earlier never separated it from the field/deref gap it happened to sit
next to; now that the latter is closed, the two are visibly different
sizes of problem, not one:

| Gap | Blocks | Where it already shows up in this document |
|---|---|---|
| Bare `Expr::Alloc`/`Expr::Boxed`/`Expr::Unboxed` (single-value allocation, arena or heap) | `tests/accept/arena_roundtrip.ls` — this backend's own boundary fixture, no `benches/` program | §7.6, §7.8 |
| `arg_count` | `benches/game/{binarytrees,fannkuch}.ls` | §7.3 |
| `Type::Float` | `benches/game/spectral.ls`, `fasta.ls`'s two `float`-filled `alloc_slice` calls | §7.3 |
| Matching through a reference (`Stmt::Match`'s `by_reference`) | No `benches/` program reaches it yet | §5's fourth slice; named separately from field/deref access in §7.11 |

Three gaps with a `benches/` program actually behind them, plus one
with none yet. `Type::Float` is now the only row blocking more than one
program (`spectral.ls` and `fasta.ls` both); `arg_count` blocks two of
its own. Closing either finishes a `benches/` program outright, the
same way §7.9 and §7.11 each did.

### 7.13 `arg_count`/`arg`, closed — and `fannkuch.ls` builds, `binarytrees.ls` does not

`argc`/`argv` as `main` was handed them, stashed once into module-local
storage (`@lexs_argc`, `@lexs_argv`, both `internal global`) before the
entry function's own body runs — `lex-sys-codegen`'s own `emit_c_main`
does the identical thing a globals-table-and-`Linkage::Local` step
apart (`docs/arguments.md` §3). `arg_count` is a load; `arg` is the
same bounds check every other indexing operation in this backend
already makes (`index >= argc` traps, one `icmp uge` covering both a
negative index and one past the end), then `argv[n]` read back and its
length computed with libc's own `strlen` — the C interface's NUL is not
part of the value this backend hands back, matching `docs/
arguments.md` §3.2.

**Both of §7.12's named targets were tried, and only one of them
builds.** `fannkuch.ls` builds, runs, and matches Cranelift exactly,
both with no argument (the fallback path) and with one (`arg_count`
and `arg` together, checked against a real `8`, not only assumed from
reading the code). `binarytrees.ls` still refuses — past `arg_count`/
`arg` it reaches `build`'s own `box[h](Tree::Node { .. })`, bare
`Expr::Boxed`, §7.12's other still-open row and not touched here. The
same shape §7.9 found with `revcomp.ls` and §7.6/§7.8 already named for
this one: a gap can block more than one program and close only some of
them, and the table has to say which.

`tests/accept/arguments.ls` — a ready-made fixture, never built
through this backend before — gets a crate-level test (no arguments,
its own contract) and a CLI differential test (also no arguments, the
"one argument, its own name" path). `fannkuch.ls` gets its own CLI
differential test passing a real `8`, the first test in this module to
give a compiled program an actual argument rather than only piped
stdin or none at all.

**Measured** (`fannkuch(11)`, two runs): `--backend llvm` is
**22%–28% faster** than Cranelift, in the same range every other
kernel in this document has landed in. `objdump` finds 16 SIMD
instructions in the object — fannkuch's own array rotation is not the
loop shape `scan`/`sieve` vectorise on, so this is a smaller number
than those, consistent with §7.11's own reading of `revcomp.ls`'s
similarly modest count: how much a kernel vectorises depends on its
own loop shape, not only on whether its traps are observable.

### 7.14 What still blocks the rest of `benches/`, updated again

`arg_count`/`arg` moves out of §7.12's table, closed — but
`binarytrees.ls` does not move with it, since bare `Expr::Boxed` was
always the bigger of the two gaps standing between it and this
backend, `arg_count`/`arg` merely the first one reached:

| Gap | Blocks | Where it already shows up in this document |
|---|---|---|
| Bare `Expr::Alloc`/`Expr::Boxed`/`Expr::Unboxed` (single-value allocation, arena or heap) | `tests/accept/arena_roundtrip.ls`, `benches/game/binarytrees.ls` (`build`'s own `box[h](Tree::Node {..})`, found in §7.13) | §7.6, §7.8, §7.13 |
| `Type::Float` | `benches/game/spectral.ls`, `fasta.ls`'s two `float`-filled `alloc_slice` calls | §7.3 |
| Matching through a reference (`Stmt::Match`'s `by_reference`) | No `benches/` program reaches it yet | §5's fourth slice; named separately from field/deref access in §7.11 |

Two gaps with a `benches/` program behind them, one with none yet.
`Type::Float` is still the only row blocking more than one program;
bare `Expr::Alloc`/`Expr::Boxed`/`Expr::Unboxed` now blocks a real
`benches/` program for the first time, not only this backend's own
boundary fixture.

### 7.15 Bare `Expr::Alloc`/`Expr::Boxed`/`Expr::Unboxed`, closed — `binarytrees.ls` builds too

Single-value allocation, arena or heap: `alloc[a](value)` is `bump`
plus one `store_leaves` call, the exact helper `alloc_slice` already
opened (§7.5) minus its fill loop; `box(h, value)` is `boxed_slice`'s
own `malloc`-and-null-check minus its fill loop; `unbox(h, b)` is one
`load_leaves` followed by one `free`, the load ordered first because a
freed pointer is not a valid read afterwards — `lex-sys-codegen`'s own
`alloc`/`boxed`/`unboxed` (`body/memory.rs`) are the identical shape,
each one a smaller version of a primitive this backend had already
built for a slice's many elements. All three share one new
`value_bytes` helper (`leaves_of(ty).len() * 8`), the whole-value
counterpart of `stride_of`'s per-element version — a whole value is
never a bare `byte` the way a slice's element can be, so it needs none
of that function's special case.

**`tests/accept/arena_roundtrip.ls`** (`alloc`) and **`tests/accept/
box_roundtrip.ls`** (`box`/`unbox`) were both ready-made fixtures,
checked against Cranelift byte-for-byte, each getting a crate-level
test and a CLI differential test. **`binarytrees.ls` — §7.13's other
named target, left refusing there on `build`'s own `box[h](Tree::Node
{..})` — now builds too**, checked both with no argument (the fixture's
own `//~ STDOUT` depth) and with a real one, matching Cranelift exactly
in both cases and exercising `alloc`/`box`/`unbox` together with
`arg_count`/`arg` rather than either capability alone. §7.13's own
finding — a gap can block more than one program and close only some of
them — resolves the other way this time: the *second*, deeper gap it
found closes too, and both of its named targets now build.

Not measured for performance: `binarytrees.ls` is allocation-and-
freeing-bound rather than arithmetic- or memory-scan-bound, the kind of
kernel this document has not yet built a `--with-malloc` comparison
for, and `objdump`'s own SIMD count is not the interesting number for a
kernel with no loop body to vectorise. A future slice measuring
allocator-bound kernels specifically is left to `docs/llvm-backend.md`
§6's own "Open" list rather than invented here.

### 7.16 What still blocks the rest of `benches/`, updated again

Bare `Expr::Alloc`/`Expr::Boxed`/`Expr::Unboxed` moves out of §7.14's
table, closed — and unlike §7.13's `arg_count`/`arg`, this one takes
both of its named `benches/` targets with it, not only one:

| Gap | Blocks | Where it already shows up in this document |
|---|---|---|
| `Type::Float` | `benches/game/spectral.ls`, `fasta.ls`'s two `float`-filled `alloc_slice` calls | §7.3 |
| Matching through a reference (`Stmt::Match`'s `by_reference`) | No `benches/` program reaches it yet | §5's fourth slice; named separately from field/deref access in §7.11 |

One gap left with a `benches/` program behind it, one with none yet.
`Type::Float` is now the only thing standing between this backend and
the rest of the Benchmarks Game suite in `benches/game/` — closing it
finishes both `spectral.ls` and `fasta.ls` at once, the same way
`arg_count`/`arg` very nearly did with `fannkuch.ls`/`binarytrees.ls`.

### 7.17 `Type::Float`, closed — and a structural gap the surface reading missed

`float` is one leaf, `LKind::F64`, exactly like `int` — but the
straightforward reading of that fact ("add an `F64` arm to a few
matches") turned out to be wrong before any code was written. Cranelift
knows an operand is a `float` **dynamically**, from the SSA `Value`
itself (`self.builder.func.dfg.value_type(a) == types::F64`), because
every Cranelift value is intrinsically typed. This backend's `LValue`
is not: it is a bare `Const(i64) | Reg(String)`, with nowhere to keep a
type tag, and `Expr::Bin`'s own IR node carries no type annotation
either. `binop`/`compare` had no branch point to detect a `float`
operand and take a different path at all.

The fix is `scalar_kind`, a new structural helper: given an
expression, it answers what `LKind` its value has **without evaluating
it** — walking the same shape the type checker already agreed on
(`Expr::Load` reads a slot's own kind, `Expr::Bin` recurses into `lhs`
unless the operator is a comparison, `Expr::Call` reads a builtin's
fixed return kind or a function's declared one, and so on), erring
rather than guessing on anything it does not recognise. `binop` and
`Expr::Neg` call it once, ahead of evaluating their operands, to
choose between the checked-`int` path already built and the new
unchecked-`float` one.

**Once that existed, the rest was direct**, each piece checked against
`docs/floating-point.md`'s own contract:

- **Literals** (`Expr::Float(bits)`) are stored as bits already, so
  `LValue` gets a new `FConst(u64)` variant printed in LLVM's hex float
  syntax (`0x3FF0000000000000`, not `1.0`) — the constant this backend
  emits is the bit pattern the parser read, never a decimal round-trip
  through `f64`'s `Display`.
- **Arithmetic** (`fadd`/`fsub`/`fmul`/`fdiv`) is never checked — a
  `float` has no analogue of the traps `checked_arith`/`checked_div`
  guard against, "the whole of §2.1's argument in code."
- **Comparison** is the ordered `fcmp` predicates (`oeq`/`olt`/…),
  false whenever either side is NaN, except `!=`, which is `une`
  (unordered-or-not-equal) rather than the ordered `one` — the one
  place `==`'s and `!=`'s IEEE semantics are not simple negations of
  each other, the same riddle `is_nan`'s own `x != x` is named for.
- **`float_of`** is `sitofp`, unchecked. **`truncate`** has no direct
  LLVM counterpart — `fptosi` is *poison* on NaN/±inf/out-of-range,
  unlike Cranelift's `fcvt_to_sint`, which traps in hardware, and
  `llvm.fptosi.sat` is the saturating form the design doc already
  calls the wrong, silently-incorrect answer — so `truncate` is three
  explicit checks ahead of the instruction (NaN, `x ≥ 2^63`,
  `x ≤ -2^63`, the second bound chosen to include exactly `-2^63`
  even though it is representable, matching Cranelift's own reasoning
  about `cvttsd2si`'s "integer indefinite" collision), the same
  ahead-of-the-instruction shape `checked_div` already uses.
- **`bits_of`** is `bitcast` plus a `select` over an `fcmp uno`
  NaN-test, canonicalising every NaN to `0x7ff8000000000000` — x86-64
  and aarch64 disagree on the sign bit `0.0 / 0.0` produces, so a bare
  reinterpretation would be the one target-dependent value in the
  language.
- **`sqrt`** is one call to `@llvm.sqrt.f64`, correctly rounded by
  construction, declared unconditionally in the module header
  alongside the checked-arithmetic intrinsics.
- **`Expr::Neg`**, found unhandled entirely while building this (it
  fell into the catch-all refusal — nothing in the accepted-test suite
  had exercised non-constant negation before now): `fneg` for `float`,
  total including on NaN and on zero, where it produces `-0.0`; `0 -
  x`, checked, for `int`, the one place integer negation overflows.
  Unrelated to floats themselves, but the same dispatch point
  `scalar_kind` was built for, so closed alongside rather than left as
  a fourth thing this document would otherwise have to reopen later.

**Both of `Type::Float`'s named targets build and match Cranelift
exactly**: `spectral.ls` (accumulation loops, `float_of`, `sqrt`) and
`fasta.ls` (many-digit decimal literals not exactly representable in
binary, and a real runtime float comparison —
`cumulative[idx] < r` — picking a base, not just arithmetic), the
second checked against the Benchmarks Game's own published output the
same way `revcomp.ls` was in §7.11.
`tests/accept/floating_point.ls` — the design doc's own dedicated
fixture, literals through `bits_of`/`is_nan` and the sign of `-0.0` —
matches too, byte for byte. It could not become this crate's own
crate-level test the way every other closed gap's fixture has,
because it `import`s `std.io` for its printing and the crate-level
harness's bare `lex_sys_ir::lower` has no `--std` source injection to
resolve that; a self-contained program threading an unfoldable runtime
value through `float_of`/`truncate`/`sqrt`/`bits_of`/`is_nan` stands in
for it there instead, and the CLI differential suite covers the real
fixture directly.

**Measured**: `spectral(1500)` is **45%–56% faster** on `--backend
llvm`, `objdump` finding 62 SIMD instructions; `fasta(1000000)` is
**10%–20% faster**, 46 SIMD instructions — both real vectorisation,
neither the checked-arithmetic ceiling this document has measured
against until now, because `float` arithmetic was never checked to
begin with.

**One more consequence, unrelated to what floats compute**: this
slice's own diff pushed `emit.rs` past `CONTRIBUTING.md`'s 2,000-line
file budget. Split by concern, the same shape `lex-sys-codegen`'s own
`body/` directory already is: `body/mod.rs` (the shared `FuncEmitter`
scaffolding — `new`, `emit`, `stmts`), `body/arith.rs` (`scalar_kind`,
`binop` and everything checked/unchecked arithmetic), `body/control.rs`
(`if`/`while`/`match`/`borrow`), `body/memory.rs` (allocation and
addressing), `body/expr.rs` (every other `Expr` and every call). A
pure reorganisation — no line of logic changed, confirmed by rebuilding
every fixture and `benches/` program this document already tracks
before and after.

### 7.18 What still blocks the rest of `benches/`

`Type::Float` moves out of §7.16's table, closed. One gap remains, and
it has had no `benches/` program behind it since §5 first named it:

| Gap | Blocks | Where it already shows up in this document |
|---|---|---|
| Matching through a reference (`Stmt::Match`'s `by_reference`) | No `benches/` program reaches it yet | §5's fourth slice; named separately from field/deref access in §7.11 |

Every `benches/` program this document tracks now builds on
`--backend llvm`. What is left is a feature nothing in `benches/`
happens to need: a match arm binding a *pointer into the scrutinee*
rather than a copy of it (`docs/reading-references.md`'s address-only
binding mode), still its own address arithmetic over an enum's variant
layout, not yet built. `tests/accept/match_a_reference.ls` is this
backend's own boundary fixture for it now, the same role
`arena_roundtrip.ls` and `floating_point.ls` each held in turn.

### 7.19 Matching through a reference, closed — every documented gap is now closed

Unlike every other slice in this document, this one was close to what
it looked like on the surface — because the two slices that came
before it had already built everything it needed. §7.11 built the
`getelementptr`-address idiom (`Expr::FieldAddr`: compute an address,
don't load); §5's fifth slice built `variant_layout` and the by-value
half of `bind_payload`. Matching through a reference turned out to be
those two facts composed, not a third thing.

**The whole change is in `body/control.rs`.** A reference is always
one pointer leaf (`Type::Ref`'s own rule in `leaves_into`), so the
scrutinee evaluates identically in both modes — only reading the tag
out of it differs: by value it is already the loaded tag; by reference
it is the scrutinee's own address, and the tag is one more `load i64`
away. `bind_payload` gained the mirror image: by value, each bound
leaf is copied out of already-loaded values; by reference, a binding
gets the *address* of its payload position instead —
`variant_layout`'s own leaf-offset, scaled to bytes and added to the
scrutinee's pointer via `getelementptr`, then that address (not a
value) stored into the one-leaf slot a reference always occupies. A
`_` binding still advances past its payload position in both modes;
there is simply nowhere to put the value, or the address. One
difference from Cranelift's own `debug_assert_eq!` on the "every
by-reference binding is one leaf" invariant: this backend returns an
`Err` instead, matching its own no-panic convention rather than a
panic that would only fire in debug builds.

**Both fixtures this document already knew about build and match
Cranelift exactly.** `tests/accept/match_a_reference.ls` — read three
times through a shared reference, freed once — matches byte for byte.
`examples/tree.ls`, the richer target `docs/reading-references.md` §4
names by name: a three-field variant (`Box[Tree]`, `int`, `Box[Tree]`)
matched by reference three separate ways — `contains` binds and
recurses through all three; `deepest` discards the first position with
`_`, exercising the offset bookkeeping past a skipped payload with an
asymmetric variant shape; `tally` binds and recurses through all
three, folding the results into a multi-leaf `struct Walk` return —
matches too. Neither fixture needed anything beyond what `bind_payload`
and `match_stmt` already gained. A third case, a `&!` unique-reference
match writing through a bound payload (`*n = *n + 1`), has no fixture
in `tests/accept/` or `examples/` — the address arithmetic does not
distinguish shared from unique, so this was checked by hand instead,
in this slice's own session: it builds, runs, and matches Cranelift.

**Every gap this document names a `benches/` or `tests/accept/` target
for is now closed.** The "outside this backend" boundary fixture moves
a fourth time: `tests/accept/bytes_to_c.ls`, refusing on a foreign
call (`extern fn`) — `Ffi`/`extern fn` was always the next-named gap in
`lib.rs`'s own module header, just never connected to a fixture until
matching through a reference stopped being in the way of naming it.
What remains unbuilt — `Ffi`/`extern fn`, `Net`, and the handful of
builtins beyond the ones this document's fifteen slices have closed —
has no `benches/` program or `tests/accept/` fixture asking for it
today; closing any of them is a future slice with no forcing function
behind it yet, the same position bare `alloc`/`Type::Float`/matching-
through-a-reference each held before something connected them to a
real target.

### 7.20 `listen`/`accept`, closed — the first crack in `Net` itself

Not a `benches/`-driven slice like every one before it: nothing in
`benches/` or `tests/accept/` asked for `Net`, so this one was scoped
directly instead, smallest sub-piece first, ahead of `connect`/`bind`
rather than after them.

`docs/listen.md` §6 is why `listen`/`accept` are the smallest piece of
the four: neither one takes a capability. A fd's authority is proved
once, at `bind` (`net_in`'s bound is checked by equality against the
capability's own bound before the syscall runs); `listen`, `accept`,
and every later `read`/`write`/`close` on the same fd take a plain
`int` and nothing else — the same reasoning `docs/net.md` §4.1 already
gives for why the resolver is a builtin rather than a lex-os facility.
That makes both ordinary fixed-signature `libc` calls, no different in
shape from `Sqrt`: narrow the `int` argument(s) to `i32`, `call`,
`sext` the `i32` result back to `i64`. `emit.rs` gained two
unconditional `declare`s (`declare i32 @listen(i32, i32)` and
`declare i32 @accept(i32, ptr, ptr)`, the trailing `ptr null, ptr null`
the same "the peer address is ignored" choice `examples/serve/serve.ls`
already made by hand); `body/expr.rs` gained the two match arms,
mirroring `lex-sys-codegen`'s own `body/expr.rs` arms line for line.

**What this slice did not build, and could not have without building
far more first:** a *real* bound fd. `bind` is a dedicated `Expr::Bind`
IR node (not a `Callee::Builtin` call), and this backend still refuses
it outright — confirmed empirically, not assumed: pointing a real
`edition 2;` program's `bind(n, port)` at `--backend llvm` produces the
documented "not part of the LLVM backend yet" refusal, naming `Bind`
specifically, while the same source runs to completion on
`--backend cranelift`. `Ffi`/`extern fn` is refused too (§7.19's own
finding), so there is no back door to a real socket either —
`examples/serve/serve.ls`'s own hand-rolled `socket`+`bind`+`listen`+
`accept` sequence, which is how a real fd gets tested against
Cranelift, is not reachable from this backend at all yet.

**Tested the other side instead.** `tests/accept/
listen_accept_bad_fd.ls` calls `listen(999, 16)` and `accept(999)`
against a deliberately invalid fd — never opened by anything — which
fails the same way, `EBADF`, on any host, with no real socket and no
live connection required. Both backends agree (`backends.rs`); the
LLVM path also gets its own self-contained crate-level test
(`crates/lex-sys-codegen-llvm/src/tests.rs`), the same "fixture plus
differential plus crate-level" trio every earlier slice left behind.

**Still refused:** `connect`, `bind`, and `Ffi`/`extern fn` — three of
`Net`'s four builtins, and the only way to obtain a real fd at all, so
`listen`/`accept` remain untestable here against a real, successful
accept until at least one of the other two lands. `bind` is next,
smallest-first (`connect`'s `getaddrinfo`-based host resolution is the
larger of the two remaining pieces, per `docs/connect.md` and
`crates/lex-sys-codegen/src/body/net.rs`'s own ~150 lines); no forcing
function names an order beyond that.

### 7.21 `bind`, closed — a real fd, and the first slice with no `phi`-free shortcut

`bind` folds `socket`, `setsockopt(SO_REUSEADDR)` and `bind` into one
call, building the same `struct sockaddr_in` `lex-sys-codegen`'s own
`bind` builds by hand and `examples/serve/serve.ls` builds by hand
again — family bytes (`2, 0`, the same "BSD reads family `0` as
`AF_INET` too" fact `docs/connect.md` §3 already measured, so no
platform branch is needed here either), the port big-endian, then
`INADDR_ANY`. `emit.rs` gained four more unconditional `declare`s
(`socket`, `setsockopt`, `bind`, `close`); the byte-store loop reuses
the same `getelementptr i8`/`store i8` idiom `Expr::Bytes` and the
arena code already established.

**The one genuinely new shape:** `socket`/`bind` can each fail, and a
failure returns `-1` rather than trapping — only a bound mismatch
traps, checked first, before any syscall runs, the same order
`lex-sys-codegen`'s own `bind` checks it in. Every slice before this
one that needed a value conditional on a runtime test either trapped
(never returning, so nothing to merge) or was a `Stmt`-level `if`
writing into an already-`alloca`'d `var`. `bind` is the first
*expression* needing a value that depends on which of three paths ran
— `body/net.rs`'s new `store_byte` plus a plain `alloca i64` result
cell, written in each of the three blocks and loaded once at the
merge label, following the same "no `phi`, a local is already memory"
rule `if_stmt`/`while_stmt` established rather than introducing a new
one.

**Checked against a real accepted connection, not only against a
description of one.** `crates/lex-sys/tests/conformance/backends.rs`
gained `the_two_backends_bind_and_accept_a_real_connection`: for each
backend, build a listener that binds a free loopback port, `listen`s,
and `accept`s, spawn it, connect a real `TcpStream` from the test
process, and check the process exits `0`. `--backend llvm` could not
have passed this before this slice — there was no way to get a real
fd out of it at all, `Ffi`/`extern fn` and `connect` both still being
refused — so this is the first Net-capable program this backend has
ever actually run, not a bad-fd stand-in like §7.20's own fixture.
`read`/`write` on the accepted connection are left out on purpose:
covering them needs `extern fn`, and adding it here would test a
boundary this slice does not touch. The wrong-port trap
(`docs/listen.md` §6.1) is checked at the crate level instead
(`binding_the_wrong_port_traps_with_sigill`), by signal rather than
only by `status.code() == None`, the same discipline every other trap
test here already follows — Cranelift and this backend agree,
`SIGILL` both times.

**Still refused: `connect` and `Ffi`/`extern fn`.** `connect` needs
`getaddrinfo`-based host resolution, genuinely larger than anything
`bind` needed; `Ffi`/`extern fn` remains the boundary named since
§7.19. Both are what stand between this backend and a program that
reads or writes what it accepts, not only accepts it.

### 7.22 `connect`, closed — the last of `Net`'s four builtins

Genuinely the larger of the two remaining pieces, as §7.21 predicted:
`checked_host` (mirroring `lex-sys-codegen`'s own function of the same
name) is a loop, not a straight-line byte-store sequence — copying the
dialled name into a 256-byte stack buffer while checking, byte by byte,
that the prefix inside the capability's bound matches, and NUL-
terminating the result for `getaddrinfo`. Built the same "no `phi`" way
every loop in this backend already is: a cursor in an `alloca i64` cell,
advanced and reloaded each iteration, rather than a block parameter —
`body/memory.rs`'s own arena-fill loop is the precedent, not a new one.

`connect` itself reuses `checked_host`, then builds a `struct addrinfo
hints` (IPv4, TCP, everything else zeroed) on the stack, calls
`getaddrinfo`, patches the resolved `sockaddr`'s port bytes the same
big-endian way `bind` already does, and calls `socket`/`connect`. Three
independent failure points — resolution, `socket`, `connect` itself —
none of which trap, so `bind`'s own "one `alloca i64` result cell,
written in every branch, loaded once at the merge label" shape carries
over unchanged, now with three failure paths funnelling into it instead
of two. `store_field`/`load_field` are `store_byte`'s general form,
needed once a struct has `i32` and `ptr` fields alongside `i8`s.
`emit.rs` gained three more unconditional `declare`s: `getaddrinfo`,
`freeaddrinfo`, `connect`.

**Checked against a real accepted connection, both directions.**
`crates/lex-sys/tests/conformance/backends.rs`'s
`the_two_backends_connect_to_a_real_listener`: for each backend, build a
client that `connect`s to a plain `std::net::TcpListener` (standing in
for the peer, since `connect` does not care what accepted it) and check
both the client's exit code and that the accept actually completed —
isolating the half `connect` adds over `bind`, not re-checking `bind`'s
own machinery. Manually, this slice's own session also built an
all-LLVM pair — a `bind`/`listen`/`accept` listener and a `connect`
client, both `--backend llvm`, talking over real loopback — the first
time two programs built by this backend have ever talked to each other.
The two bound-mismatch traps (`docs/connect.md` §10.1: host outside the
prefix, then the exact-port check) are checked at the crate level, by
signal, the same way `bind`'s own wrong-port trap already is — Cranelift
and this backend agree, `SIGILL` every time.

**Still refused: `Ffi`/`extern fn`**, named since §7.19. `Net` itself is
fully built on `--backend llvm` — `listen`, `accept`, `bind` and
`connect` all lower — but a program that wants to `read`/`write` what it
accepted or connected to still needs `extern fn` for that, the same way
`examples/serve/`/`examples/fetch/` do on Cranelift. Closing `Ffi`/
`extern fn` is what would let a program like `examples/seek/`'s
next-door neighbour actually move bytes over a socket on this backend,
not merely open one.

> **Correction.** This section, and this file's module-header summary in
> `lib.rs`, called `Ffi`/`extern fn` "the only gap left" three times over
> §7.20–§7.22. That was never measured — `Fs` (`fs_read`/`fs_write`/
> `open_read`/`file_read`) has been unbuilt on this backend the entire
> time, and nothing in this document named it, because no slice had
> tried building a program that used it. §7.23 found this the first time
> it checked a real `Fs`-using program (`examples/seek/`) against the
> backend it had just declared complete. Corrected here rather than
> quietly, the same way §3.3 and §7 corrected an earlier wrong claim in
> place instead of deleting the sentence that made it.

### 7.23 `Ffi`/`extern fn`, closed — and the correction finding it forced

An import under the symbol the declaration named, mirroring
`lex-sys-codegen`'s own `Callee::Extern` arm: a capability parameter
carries no data and never reaches C (`crosses_to_c`, copied over
unchanged); everything else crosses at lex-sys's own widths — an `int`
is `i64` here whatever the C function's own parameter width is, the same
choice `lex-sys-codegen`'s own `emit.rs` makes and `docs/reach.md` §3
documents. `emit.rs` gained a new pass declaring one `declare` per
distinct symbol in `program.externs`, deduplicated (`docs/modules.md`
§3: an `extern fn` name is scoped to its module, but the C symbol is
not, so two declarations can share one) — computed with the same
`leaves_of`/`crosses_to_c` pair the call site uses, so the two can never
disagree. `Type::Unit` (no `-> Type` written) reads as `void`; it is
otherwise unwritable, so this is the one place it is read rather than
produced.

**The call site reuses `Callee::Fn`'s own call-and-unpack shape**,
factored out as `emit_call` once `Callee::Extern` needed the identical
"`void` to nothing, one leaf to a register, more than one to
`extractvalue`" logic — a real, if small, simplification found while
building this slice rather than a plan going in.

**One structural gap surfaced immediately: `scalar_kind`** (§7.17) had
no arm for `Expr::Call { callee: Callee::Extern(_), .. }`, so a foreign
call's result could not be told apart from a `float` — needed the
moment a program built here uses one in an expression rather than only
as a statement. Closed the same way `Callee::Fn`'s own arm already was:
the declared return type's leaf, read structurally rather than
evaluated.

**Checked against `tests/accept/bytes_to_c.ls`, not a fixture written
for this slice.** It was already checked in — GNU's own `write(fd, ptr,
len)`, both a string literal and an arena-allocated slice crossing as
the pointer-and-length pair `docs/strings.md` §6 describes — and its
existing `//~ STDOUT`/`//~ EXIT` directives are now met on
`--backend llvm` unchanged. `backends.rs`'s new
`the_two_backends_agree_on_bytes_to_c` checks both backends against it;
`crates/lex-sys-codegen-llvm/src/tests.rs`'s new
`extern_fn_labs_computes_the_real_answer` is the plain-`int` case,
checked against the real answer (`labs(-5) == 5`) rather than only
against "it built".

**What checking real programs against this slice actually found: two
things this document had not named.**

1. **`Fs` is still unbuilt**, per the correction above — found trying
   `examples/seek/`'s own `read_file` against `--backend llvm` and
   getting `` `OpenFile { .. }` is not part of the LLVM backend yet ``.
   The "outside this backend" boundary fixture moves a sixth time, from
   `tests/accept/bytes_to_c.ls` (now inside) to `tests/accept/
   file_handle.ls`, refusing on `fs_write`'s own `FileOp` — checked at
   the crate level and through the CLI, the same two ways every earlier
   move was.
2. **A user-declared `extern fn` can collide with this backend's own
   fixed libc declarations.** `examples/serve/serve.ls` declares
   `extern fn socket[&f](ffi, domain: int, kind: int, proto: int)`,
   crossing every `int` at `i64`; `bind` (§7.21) already declared
   `@socket` unconditionally at libc's own `i32` width, for its own
   internal use. Two declared signatures, one linker symbol — `clang -c`
   correctly refuses the module (`invalid redefinition of function
   'socket'`), a located, non-crashing refusal, not a silently wrong
   build. **Not a new bug**: `docs/ROADMAP.md`'s own entry for #92
   already recorded the identical exposure on Cranelift — `close`
   declared at
   two different widths by `bind`'s own failure path and by a program's
   `extern fn close`, "not fixed \[t\]here \[...\] a known exposure
   shared with `open_read`/`fs_read`/`fs_write` since before `Net`
   existed." This slice makes the same exposure visible on
   `--backend llvm` too, for the same reason and left unfixed for the
   same one: none of `serve.ls`/`fetch.ls`/`report.ls`/`collect.ls`
   needs a *different* `socket`/`bind`/`connect`/`listen`/`accept`/
   `setsockopt`/`close` than this backend already declares for `Net`
   itself, so the fix is narrowing what each program actually asks
   for, not this backend's declare list — the same shape the Cranelift
   note already reached for the same reason.

Between the two: `Ffi`/`extern fn` lowers, and `docs/agent-tools.md`'s
own `examples/seek/` is closer to portable across both backends than it
was, but `Fs` is what actually stands between it and `--backend llvm`
today — not `Ffi`, which this document spent three slices calling the
last gap before checking.

### 7.24 `Fs`, closed — and a comparison bug three slices had never reached

`checked_path` (mirroring `lex-sys-codegen`'s own function of the same
name) is `checked_host`'s loop with two things added: a `/`-boundary
check on the way out (`/tmp` does not contain `/tmpevil`) and a `..`-
traversal refusal inside the loop, both built the same "no `phi`,
`trap_if` on the escape" shape `connect`'s own loop already established
— not a new mechanism, one loop body slightly larger than the last.
`file_op` (`fs_read`/`fs_write`) and `open_file` (`open_read`) both
reuse it; `read_file` (`file_read`) loads the handle's one leaf and
calls `read`. `errno` — a *function* in every modern libc, because it
has to be per-thread — is one more platform-symbol split
(`__errno_location` on glibc, `__error` on Darwin), the same shape
`stdout`/`stderr`'s own split already is. `emit.rs` gained a new
`Type::Named(def, _) if def.0 as usize == PRELUDE_FILE` arm: a file
handle is one `i64` leaf and nothing else, the same special case `Box`
already has, needed because `File` is an opaque prelude type
`program.type_info` has no entry for — `Opened`/`Read` need no such
arm, being ordinary prelude *enums* the general `TypeInfo::Enum` case
already scalarises correctly.

**Checked against `tests/accept/file_handle.ls`, the boundary fixture
itself.** Unmodified, it now builds and runs on `--backend llvm`,
matching its own `//~ STDOUT` directives exactly: `fs_write` writes a
file, `open_read` opens it, `file_read` reads it twice — once for the
twelve bytes, once past the end, proving `Read::End` answers `End`
*again* rather than repeating the last byte count — and `file_close`
ends it. `backends.rs`'s new `the_two_backends_agree_on_file_handle`
checks both backends against it.

**One collision found immediately, and fixed rather than added to the
known-exposure list.** `read`/`write`, declared unconditionally for
`Fs`'s own use, broke `tests/accept/bytes_to_c.ls` — the very fixture
§7.23 had just proven — because that program declares its own `extern
fn write`, crossing every `int` at `i64`, where `Fs`'s own `write` call
needs libc's true 32-bit `fd`. Unlike `socket`/`bind`/`connect`'s own
already-documented exposure (§7.23, `docs/ROADMAP.md` #92), this one
regressed a program that had been *working* a slice ago, not one that
had never worked — so `read`/`write` are now declared only when
`program.externs` does not already claim the symbol, letting a
program's own declaration stand in for this backend's internal one.
`open`/`creat` are not guarded: no example declares either for itself,
so there is nothing yet to collide with, and guarding everything
unconditionally would be the cross-cutting fix `docs/ROADMAP.md` #92
already declined to make, not this slice's to make unasked.

**The second finding is not about `Fs` at all.** Building
`examples/cut/` and `examples/seek/` against this slice — both reach
`Fs` only incidentally, through `std.flags`'s own argument parsing —
found `clang` refusing the emitted module with an ill-typed comparison:
`%t20` (an `i8`) compared as `i64`. `compare` had always hardcoded
`icmp {cc} i64` regardless of what its operands actually are, silently
correct for `int` — already `i64` — and silently wrong for `byte`/
`bool` (`i8`), and nothing had caught it in eighteen prior slices
because nothing this document tracks compares a raw `byte`:
`std.bytes.compare` converts through `int_of` first, `sieve`/`scan`/
`fannkuch` compare `int`s throughout, and `match`'s own tag comparisons
are already `i64`. `std.bytes.find`'s `text[at + i] != needle[i]` —
called by `std.flags.named`, called by both `cut.ls` and `seek.ls` — is
the first comparison this document's own suite reaches that is not.
Fixed by threading `binop`'s already-computed `lhs_kind` through to
`compare`, the same value `float_binop` was already being dispatched on
one line above it. `tests/accept/bytes_to_c.ls` did not trip this only
because a `&r [byte]` crossing to C never compares its bytes to
anything; `examples/cut/`/`examples/seek/` compare them constantly.
Checked directly: `byte_comparison_uses_the_right_width`
(`crates/lex-sys-codegen-llvm/src/tests.rs`), `!=` and `==` on two
runtime `byte`s an `int` literal cannot fold away.

**The "outside this backend" boundary fixture moves a seventh time**,
from `tests/accept/bytes_to_c.ls` (now inside, since §7.23) to
`tests/accept/static_data.ls`, refusing on `Expr::Static` — found the
same way `Fs` was, by checking whether this backend's own `Expr` match
was actually exhaustive rather than assuming it, once every named gap
this document had tracked was closed. `docs/compile-time-data.md`'s
whole feature — a `static` block evaluated at compile time, its result
read as ordinary data — has no `--backend llvm` support yet, and no
slice before this one had tried a program that used one.

### 7.25 `Expr::Static` and `Expr::BitNot`, closed — and the boundary that ran out of fixtures to move to

`Expr::Static`'s data does not need building twice: `lex-sys-ir`
already evaluates every `static` at compile time, into
`Program::statics: Vec<StaticValue>` — one entry per `static`, holding
its element type and its values as `i64`s (a `float`'s bits, the same
shape `Expr::Float` already holds one in). Both backends read the same
`Vec`; only laying it out and referencing it is backend work.
`emit_module` gained a loop mirroring `lex-sys-codegen`'s own
`emit.rs`: one `private unnamed_addr constant [N x i8]` global per
static, packed at `stride_of`'s own stride — one byte per element for
`Type::Byte`, eight for `int`/`bool`/`float`, the same rule
`alloc_slice`'s own byte-copy already follows, so a `[byte]` static
comes out packed the same way a string literal or any other byte slice
does. `Expr::Static` itself is then only a reference to an
already-declared symbol: `@lexs_static_{name}` and the element count,
the same `(pointer, length)` pair every slice in this backend is.
Unlike a string literal, whose `bytes_lit` declares a fresh global at
every occurrence (`docs/strings.md` §8 leaves interning open), a
static's global is declared exactly once in `emit_module`, because a
512 KB table is not a greeting: two readers of the same `static` get
two references to the one symbol, never two copies of the table.

**Checked against `tests/accept/static_data.ls`, the boundary fixture
itself, unmodified.** It builds and runs on `--backend llvm`, matching
Cranelift byte for byte: a table built by a `while` loop, a `[byte]`
table shifted through `int_of`/`byte_of`, a second `[int]` table built
by calling a pure function (`twice`) against the first — proving a
`static` body may call an ordinary function, the same evaluator that
folds `factorial(5)` running it — and a pure function (`square_of`)
that reads a `static` directly, with no parameter threading it in.
`backends.rs`'s new `the_two_backends_agree_on_static_data` checks the
same claim through the CLI, and `crates/lex-sys-codegen-llvm/src/
tests.rs`'s new `a_static_table_is_built_at_compile_time_and_reads_
back_correctly` checks a loop-built table numerically (`0 + 1 + 4 + 9 −
14 = 0`) rather than only "it printed the right thing", so a wrong
symbol name or a wrong stride would fail loudly rather than by
coincidentally printing something plausible.

**`Expr::BitNot` was not part of this backend at all, found while
checking whether it actually was.** Closing `Expr::Static` left this
document ready to claim, again, that every named gap was closed — the
same claim §7.20 through §7.22 made three times running before §7.23
found it false. This time the check was direct: walk every variant
`lex_sys_ir::ir::Expr` declares and confirm `body/expr.rs`'s `expr`
match has an arm for each one, rather than trusting the running list of
named gaps to be complete. `Expr::BitNot` had none. `tests/accept/
bitwise.ls` already exercises `~0` and has since this backend's early
slices, so a Cranelift-only reader would reasonably have assumed the
operator worked everywhere `bitwise.ls` does — but `~0` is two
literals, folded to `Expr::Int(-1)` by `docs/compile-time.md` §3 before
codegen ever runs, so `bitwise.ls` alone was never capable of catching
a missing `BitNot` arm on this backend. Confirmed directly: a
self-contained program applying `~` to `putchar`'s own runtime echo (a
value the checker cannot fold) hit `` `BitNot(Load(Slot(n)))` is not
part of the LLVM backend yet ``, the same located refusal every other
gap in this document produced, not a panic. `docs/bitwise.md` §1
restricts the whole operator set to `int`, so `BitNot`'s fix is one
line: the same `xor`, `%flipped`, `LValue::Reg` shape `Expr::Not`'s arm
right above it already has, at `i64` and `-1` in place of `Not`'s `i8`
and `1` — the two operators' entire difference, since `Not`'s operand
is always exactly `0` or `1` and `BitNot`'s is not.

**Checked against `tests/accept/bitwise.ls`, run against this backend
for the first time.** `backends.rs`'s new `the_two_backends_agree_on_
bitwise` matches Cranelift's output for every operator on that page —
`&`, `|`, `^`, `<<`, `>>`, the arithmetic-shift and no-trap-on-value
rules, and the precedence case — byte for byte; `bitwise.ls`'s own `~0`
had run against Cranelift alone until now (`corpus.rs`, no `--backend`
flag), never against this one. Because that fixture's only `~` use
folds away, `bitnot_flips_every_bit_not_just_the_low_one`
(`crates/lex-sys-codegen-llvm/src/tests.rs`) checks the operator on a
value the checker cannot fold: `~5` computed as `-6` and checked against
`-6` arithmetically, rather than against the `4` a mistaken "flip the
low bit" implementation (`Not`'s own shape) would have produced.

**With both closed, `body/expr.rs`'s `expr` match has no unhandled
`Expr` variant — so its wildcard arm came out.** What had been a
runtime string — `` `{other:?}` is not part of the LLVM backend yet ``
— is now nothing: `rustc` refuses to build this crate at all if a
future `Expr` variant goes unmatched, which is a strictly stronger
guarantee than the removed arm's own test ever gave. The same
enumeration run over `Callee::Builtin`'s variants found nothing left
either — every one reaches an arm in this backend's `call()`, `FsRead`/
`FsWrite`/`OpenRead` correctly absent from that list because they never
reach `Callee::Builtin`, lowered as `Expr::FileOp`/`Expr::OpenFile`
before codegen sees them, the same way `Connect`/`Bind` are.

**The "outside this backend" boundary fixture does not move an eighth
time, because this slice's own session found no real fixture left to
move it to.** Every file in `tests/accept/` and every program in
`examples/` was built against `--backend llvm`, checked directly rather
than inferred from the two enums: all of them succeed. The one program
that still refuses, `examples/collect/collect.ls` (and `fetch/`,
`report/`, `serve/` alongside it), does so for the reason §7.23 already
named and §7.24 already found a second instance of: each declares its
own `extern fn socket`, crossing at `int`'s own `i64` width
(`docs/reach.md` §3), which collides with the `i32`-parameter `@socket`
this backend declares unconditionally for its own `Net` support —
`clang` refuses to link the disagreement rather than silently picking
one. Not a new exposure: `docs/ROADMAP.md`'s #92 entry already recorded
the same shape on Cranelift for `close`, accepted there and left
accepted here for the same reason — a program naming the real libc
signature for itself needs no help from this backend's own internal
declaration of the same symbol, and guarding every libc name this
backend ever declares would be the cross-cutting fix #92 already
declined to make. `a_program_outside_this_backend_is_refused_not_
panicked` and `a_program_outside_this_backend_is_refused_through_the_
cli` now check that collision directly — still a located refusal, still
not a panic, just no longer a missing `Expr`.

What this slice actually closes, honestly stated: not "every program
this backend will ever refuse" — `docs/ROADMAP.md` #92's own collision
is proof a refusal can still be correct and permanent — but every
*missing* `Expr` or `Builtin` arm this backend's own match statements
could have, which is now zero, and checked by the compiler rather than
asserted in prose.
