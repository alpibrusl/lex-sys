# GPU, and whether `lex-gpu` should be its own language

> **Status: measured, and the measurement decides it.**
>
> The question arrived as three: can lex-sys run on a GPU, how would a
> *native* one work, and should it be a separate language. The first two
> are design; the third looked like taste until the numbers came in.
>
> §2 is the result and it has a surprise in it. **A bounds check is free
> even in vectorised code — an overflow trap is not.** So the memory half
> of this language's no-undefined-behaviour claim survives on a GPU at no
> cost, and the arithmetic half cannot survive at all.

---

## 1. Today: nothing, and the backend is why

Cranelift has no GPU target — no PTX, no SPIR-V, no AMDGPU. Emitting for
a GPU means LLVM, which `ROADMAP.md` currently places outside the next
slices and `purity.md` §4.2 argued *down* rather than up.

So this document is not about a feature that is nearly there. It is
about what the design would have to give up, priced.

---

## 2. What each guarantee costs a vectoriser

The same reduction — sum a million-element buffer, 200 rounds — written
four ways in C and twice in lex-sys, with the guards switched
independently. `benches/reduce.c` and `benches/reduce_{checked,wrapping}.ls`,
which the conformance suite holds to the same answer.

SIMD instructions counted in the emitted `run`, not inferred from the
clock. clang 18, `-O2`; minimum of nine runs:

| | SIMD in `run` | | |
|---|---|---|---|
| clang, no guards | **10** | 60.7 ms | 1.00× |
| clang, **bounds check only** | **10** | 61.4 ms | **1.01×** |
| clang, overflow trap only | **0** | 88.9 ms | 1.46× |
| clang, both | **0** | 91.1 ms | 1.50× |
| lex-sys, wrapping (bounds only) | 0 | 137.6 ms | 2.27× |
| lex-sys, checked (both) | 0 | 252.1 ms | 4.15× |

Three things fall out, and none of them was obvious beforehand.

### 2.1 A bounds check is free

**1.01×, and the SIMD count does not move.** The index condition is
provably true inside the loop the compiler already proved bounded, so it
is deleted outright. A bounds check never was a reassociation barrier —
it is a branch on a constant, and a branch on a constant is not a branch.

`overflow-cost.md` §3.2 established that *a* check costs the vectoriser.
It measured the overflow one and generalised. **Half of that
generalisation is wrong**, and this is the half: memory safety is not
what costs anything here.

> **Corrected (#64): that last sentence is the over-generalisation
> again, one document later.** [`check-cost.md`](check-cost.md) measured
> all eight loop-body checks, and memory safety is *not* uniformly free:
> `s[lo..hi]` costs **2.16×**, the most expensive integer check in the
> language. It is the same kind of check as `s[i]` and it traps the same
> way.
>
> What makes `s[i]` free is that `i < n` is the loop's own condition, so
> the compiler discharges it. Move the same two comparisons onto bounds
> loaded from memory and the cost is 2.16×; move them back onto the
> induction variable and it is 0.99×. **Provable is free, and loaded is
> not** — and this row happened to measure the one check in the language
> whose condition the loop always proves.

### 2.2 An overflow trap costs 1.46×, and it costs it as SIMD

Ten SIMD instructions to zero. This reproduces §3.2's finding on a new
kernel and a new compiler version: an addition that may trap is an
addition whose order is observable, so a reduction cannot be split
across lanes.

### 2.3 Deleting the trap does **not** reach C

This is the one that decides the third question. lex-sys without traps
is **2.27×** off vectorised C, and still emits **no SIMD at all** — it is
1.55× slower than even *scalar* clang. The trap accounts for 1.83× of
lex-sys's own cost (252.1 → 137.6); everything remaining is Cranelift.

**So removing the trap is necessary and not sufficient.** A GPU-shaped
lex-sys on the current backend would be scalar, which on a GPU is the
same as not having one.

---

## 3. How a native GPU lex-sys would work

Worth writing down, because three of the pieces already exist and cost
nothing to reuse.

**A kernel is a function whose row is empty.** A kernel may not open a
file, syscall or allocate; here that is not a convention but the absence
of `Io`, `Fs`, `Heap` and `Ffi` from a signature, and `purity.md`
measured 35% of functions already proving it. No `kernel` keyword is
needed: `launch` names the roots the way `main` already names emission
roots, and the check is on the row.

**Regions are address spaces.** `region a { … }` is a lexically scoped
arena whose references cannot escape — 11 must-reject fixtures enforce
exactly that — which is the lifetime CUDA `__shared__` has and does not
check. Same rule, same checker, nothing new.

**The device is a capability.** `Gpu(device)` beside `Fs(prefix)`,
refinable by the existing `narrow`, with `gpu_launch(d)` and
`gpu_copy(d)` as labels. `lex-sys authority` then names the device, and
a program never handed one provably never launched.

---

## 4. What it would cost the design

| | |
|---|---|
| **`&!` must mean unique** | Since measured, and it is **the expensive row** — [`aliasing.md`](aliasing.md). Not because it refuses working code (it refuses one fixture in 82 programs) but because one of the three aliasing routes, a reference returned from a call, closes only with provenance in signatures: lifetimes, and a borrow checker the non-goals exclude. GPU is what makes it mandatory rather than optional — on one thread two aliasing writes are defined and ordered, and two lanes writing through aliasing references is the race the checker should catch — so this row is where the GPU question stops being about speed |
| **Recursion** | GPUs have no stack for it; `std.io.print_nat` is recursive. Kernels become a subset, and no row can say a function is in it |
| **Barriers** | A barrier reached non-uniformly is undefined behaviour on real hardware — the one thing this family of languages refuses to have. Making that checkable is the research contribution, and lex-sys has nothing to donate to it |
| **Two backends, forever** | Cranelift stays because it is the fast dev path |
| **The trap model** | §5 |

### 4.1 The trap model is the thesis, and the hardware does not have one

`defined-behaviour.md` §2.1: an operation with no right answer **stops**.
A GPU cannot stop — no signals, no per-lane abort. Three answers:

1. **Wrapping arithmetic in kernels** — abandons the claim exactly where
   the compute is.
2. **Poison, reduced at the launch boundary** — each lane sets a flag,
   `launch` answers *some lane trapped*. The claim survives; *where* does
   not.
3. **Refuse trapping operations in kernels** — a narrower language
   inside the braces.

Only (2) keeps the thesis, and §2.2 prices what (1) and (3) are buying:
1.46× on a mature backend, which is most of the reason to own a GPU.

**But §2.1 halves the bill.** Bounds checking is free, so a kernel
dialect keeps array safety in full and gives up only overflow trapping.
That is a much smaller concession than "no undefined behaviour does not
fit a GPU" — it is *arithmetic* undefined behaviour, and it is
recoverable by (2) at a cost this document has not measured because
nothing here can run a kernel yet.

---

## 5. So: should `lex-gpu` be its own language?

**Not yet, and the reason is §2.3 rather than taste.**

The case for independence is real: a language whose failure model is
*trap* and one whose failure model is *poison* are two languages, not
one with a flag. That is the same argument that justified lex-sys
existing beside lex-lang — different layer, different job.

But the measurement says the split buys nothing where it matters.
Deleting the trap leaves lex-sys scalar; **the thing that actually buys
GPU speed is LLVM, and LLVM is equally required whether the kernels are
a lex-sys dialect or a separate language**. And the two genuinely novel
parts — checked barriers and poison-as-an-effect — are new work in
either design, because lex-sys does not have them to inherit.

What *is* worth saying about the shape: lex-sys is already six crates,
so a sibling could depend on `lex-sys-syntax` and `lex-sys-types` and
own only the dialect, the lane checker and the backend. Roughly 80% of
the front end shared and none of the back end. That is a much better
arrangement than a fork, and `ROADMAP.md`'s risk register names the
alternative: *"second-system trap — during a port every change lands
twice."* A third repository makes it three.

And the plainest argument: lex-sys's own README says **not a usable
language yet**. A third sibling before the second is usable is a
recognisable way for a project to end.

### 5.1 What would change this

| Signal | Why it would settle it |
|---|---|
| An LLVM backend exists | §2.3's remaining 2.27× is Cranelift. With LLVM, the wrapping half reaching vectorised C makes the GPU question purely about the failure model, which is a language question and answerable |
| Poison measured, not assumed | §4.1's option (2) has no number. A CPU prototype — a flag per lane, reduced at the end — is measurable on the existing bench harness and would price the thesis's survival |
| A program that wants it | Nothing in this repository is data-parallel. `benchmarks-game.md`'s kernels are the nearest, and they are single-threaded on purpose |

---

## 6. Open

| Question | Why it waits |
|---|---|
| Does poison cost less than trapping? | §5.1. The one number that decides whether the thesis survives a GPU, and it is measurable on a CPU today. [`check-cost.md`](check-cost.md) §7 **raises it**: this was a question about one check when it was written and is now about six, with the worst at 3.35× |
| Checked barriers | §4. A real research problem — roughly structured concurrency for lanes — and the part nobody else has done either |
| ~~`overflow-cost.md` §3.2's generalisation~~ | **Answered** — [`check-cost.md`](check-cost.md). Six of the eight checks this language emits in a loop body take the SIMD count to zero, not one, and the axis is neither memory-safety nor arithmetic but whether the loop already proves the condition. §2.1 above is corrected in place: it measured the one check whose condition a loop always proves |
| A host-side GPU probe through `Ffi` | The `reach.md` move: can a lex-sys program drive a GPU at all, with no new backend? It would answer a different question — reach, not speed — and would report `ffi("libcuda")` and nothing about the device, which is §5's narrowing gap in a third domain |
