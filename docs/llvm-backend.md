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
| `region`/`alloc_slice` (arena allocation, `Stmt::Region`) | `sieve_*.ls`, `scan_*.ls`, `benches/three/sieve.ls`, `benches/game/fasta.ls`, `benches/game/revcomp.ls` | §5's "later slices" list, already named |
| `wrapping_add`/`wrapping_sub`/`wrapping_mul` | every `_wrapping.ls` half of a `benches/` pair, `benches/three/purity.ls` — so **no checked-vs-wrapping overflow-cost pair builds on this backend today**, only the checked half | Implicit in §5's "every `Builtin` beyond `PutChar`/`Split`/`Release`/`Narrow`/`IntOf`"; not previously named on its own |
| Heap boxing (`box`, `box_slice`, `Contents`, `unbox`, `unbox_slice` — `Type::Box`/`BoxedSlice`) | `reduce_*.ls`, every `benches/layout/*.ls` file | Same bucket as above; not previously named on its own |
| `arg_count` (and argument reading generally) | `benches/game/binarytrees.ls`, `benches/game/fannkuch.ls` | Same bucket |
| `Type::Float` and float arithmetic | `benches/game/spectral.ls` | `emit.rs`'s own `LKind` doc comment already says floats are refused; not previously named as a *benchmark*-blocking gap |

Ordered by what it would unblock: **`wrapping_*` first** — it is the
smallest of the five (three `BinOp`-adjacent builtins, no new type, no
new `Place`), and it alone would let `sum`, `fib`'s wrapping halves and
`benches/three/purity.ls` build, making the overflow-check's own cost
(`docs/overflow-cost.md`'s question) measurable on this backend for the
first time. **Arenas second** — `sieve`/`scan`/`fasta`/`revcomp` are four
of the remaining programs behind one gap, and it is already `docs/
llvm-backend.md`'s own next-named slice. Heap boxing, `arg_count` and
float are each smaller pockets behind their own single gap, not bundled
with anything else in `benches/`.

| Bench | |
|---|---|
| `scripts/backend_compare.py` | Interleaved cranelift-vs-llvm timing on the kernels that build on both, today three; `--with-c` adds a three-way leg for `mandelbrot.ls` against `mandelbrot.c` |
| `crates/lex-sys/tests/conformance/backends.rs` | `the_two_backends_agree_on_{sum_checked,fib_checked,mandelbrot}` — not a timing gate, for the reason `every_benchmark_pair_agrees` gives |
