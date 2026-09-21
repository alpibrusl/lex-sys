# What a checked row is worth

> **Status: measured, and unspent.**
>
> The question is whether there is anything lex-sys can do *better* than
> C and Rust, rather than 1.6× worse (`against-c-and-rust.md`). There is
> exactly one candidate, it is structural rather than a matter of tuning,
> and this document measures it.
>
> **The answer: the effect row is a checked purity proof. C can only
> promise the same fact, unchecked; Rust cannot state it at all. On the
> loop measured here that fact is worth 158×, 35% of the functions in
> this repository qualify — and nothing collects it today.**

---

## 1. The fact the other two cannot have

A function's row is checked exact in both directions (`linearity-and-effects.md`
§7): performing an undeclared effect is an error, and declaring one you
never perform is also an error. So `[]` is not a hint. It is a proof,
produced by the type checker, present on every function in the language
whether or not anybody wanted it.

What the other two have:

| | Can it state purity? | Is it checked? |
|---|---|---|
| **lex-sys** | Yes — the row, on every function | **Yes**, by the type checker |
| C | Yes — `__attribute__((const))` / `((pure))` | **No.** A wrong one miscompiles silently, with no diagnostic ever |
| Rust | **No.** There is no purity attribute | — |

Rust's position is worth being precise about, because it is surprising:
LLVM *infers* `readnone` for small functions within a codegen unit, and
that inference is why `against-c-and-rust.md` measured 1.6× rather than
something worse. What Rust has no way to do is **declare** it, so the
inference stops at a crate boundary without LTO. There is no stable
`#[pure]`, and the effect systems proposed for Rust have not landed.

C's `const` is the interesting comparison. The attribute exists, it works,
and nobody writes it — because writing it is a promise nothing verifies.
That is the same attribute this language gets for free and cannot get
wrong.

---

## 2. What the row proves, exactly

> **A function is pure when its `performs` row is empty *and* no
> parameter reaches a unique reference.**

Both conditions are load-bearing.

**The row alone is not enough.** `std.vec`'s `set` declares `[]` and
writes through `&!v Vec[T]`. Memory written through a reference is not an
*effect* in this model — effects are capabilities — so a row of `[]` says
nothing about it. The second condition is what rules it out.

**And those two are the whole list**, which took checking rather than
assuming:

- A function **cannot allocate into a caller's arena.** `alloc` only
  works inside a `region r { .. }` block that is lexically open, so
  `fn build[&r](n: int) -> [] int { alloc_slice[r](...) }` is refused —
  *"`r` is not an arena open here"*. Allocation is always local and dies
  with the block.
- A shared reference is **read-only**, and `borrow` freezes what it
  borrows, so nothing can write the data out from under a second call
  during the region.
- An owned `res` argument **cannot be supplied twice**, so the case where
  purity would be unsound is the case linearity already forbids.

`Func::is_pure` is those two conditions and nothing else, and
`purity_is_the_row_plus_what_a_reference_may_do` checks it on six cases
chosen to break it.

### 2.1 How many functions qualify

Every program in the repository, counted:

```
76 pure of 217   (35%)
```

Ranging from 0 of 2 in `hello.ls` to 15 of 26 in `rational.ls`. It is not
a curiosity — it is a third of the code.

---

## 3. But purity is not "safe to hoist"

This is where a trapping language pays for one of its own commitments,
and the two halves of the prize are very different sizes.

A pure function here **may still trap**: `+` traps on overflow
(`defined-behaviour.md` §2.1), an index traps out of range, a shift traps
past the width. Moving a call out of a loop that runs zero times invents
a trap that the program would not have taken. So:

| Transformation | Needs | Measured worth |
|---|---|---|
| **CSE** — two identical calls in one iteration become one | purity alone | **1.94×** |
| **Hoisting** — a loop-invariant call leaves the loop | purity **and** "does not trap" | **158×** |

Both numbers are the same loop, the same function, in C, with and without
`__attribute__((const))`; the CSE row varies the argument so hoisting is
impossible and only the redundant second call can go.

So the headline 158× is **not** freely available to this language, and
saying otherwise would be the kind of claim this project exists not to
make. What is freely available is the 1.94×, which is still a doubling
that neither C-as-people-write-it nor Rust can reach across a
compilation boundary.

Recovering the rest needs a second predicate — *pure and cannot trap* —
which is checkable (a body using only `wrapping_*`, comparisons and
control flow cannot trap) and is filed in §6 rather than guessed at here.

---

## 4. Nothing collects it, and that is on purpose

```
$ python3 scripts/three.py        # the `purity` group

lex-sys  knows, cannot spend      0.2847s
C        knows nothing            0.1852s
Rust     cannot be told           0.1845s
C        __attribute__((const))   0.0012s
```

All four print the same checksum. lex-sys is 1.54× uninformed C, which is
the backend gap `against-c-and-rust.md` already measured — and the 158×
sits there unspent.

**Cranelift cannot express it.** There is no call attribute for "no side
effects" in `cranelift-codegen` 0.121; `readonly` is a flag on *load*
instructions, not on calls. So there is nothing to emit.

**And writing our own pass is excluded on purpose.** `README.md`'s
non-goals list "an own optimiser", and the backend plan is *"Cranelift
for dev, LLVM for release"*. A purity-driven CSE pass in this repository
would be a middle-end by another name, and the reason not to build one is
the reason the non-goal exists: it is the work that turns three months
into three years.

What this document changes is that the exclusion is now **informed**. The
list said "yes, later" without knowing what later was worth. It is worth
1.94× on every redundant pure call, 158× where the call is also
loop-invariant and cannot trap, on 35% of the functions here.

### 4.1 What would collect it

LLVM has exactly the vocabulary: `readnone` for §3's CSE row, plus
`willreturn` and `nounwind` for the hoisting row. The row and the
parameter check produce both facts with no analysis at all — the
information a C compiler must infer, or a C programmer must promise, this
language simply has.

### 4.2 And there is no lex-sys program that can show it

> **This document overstated its own conclusion on the first pass, and
> the correction is the interesting part.**

The draft said the LLVM backend would be *"the one place this language
has something to give an optimiser that the optimiser cannot get anywhere
else."* True of the language. Not reachable by any program written in it
today, for a reason that has nothing to do with purity:

**lex-sys has no separate compilation.** A program is the set of files
named on the command line, parsed into one AST (`many-files.md` §2).
There are no libraries, no `import` of a compiled unit, no linking of two
lex-sys objects. So there is no boundary for a purity fact to survive —
and §1's whole argument was about what survives a boundary.

Give that whole program to LLVM and LLVM sees every body. Its
`function-attrs` pass infers `readnone` itself, for the same functions,
without being told. The row would be **correct and redundant**.

What is left is narrower and worth stating exactly, because it is not
nothing:

- LLVM's inference is a conservative analysis that gives up — on
  recursion, on large bodies, on anything reached indirectly. The row
  never gives up, because it was checked rather than inferred.
- The row is available *before* codegen, to any consumer, including
  tools. `lex-sys authority` prints it today and no optimiser is involved.

But the 158× in §4's table came from a benchmark built with a
**deliberate** compilation boundary, in C, because that is the only way
to show the effect at all. lex-sys cannot construct that program.

**So the honest conclusion is conditional**: the row is a real advantage
over C and Rust *if* lex-sys ever gains separate compilation or a library
model — which `many-files.md` §2.1 defers on purpose — and is mostly
redundant with LLVM's own inference until then. That is a reason to file
this and move on rather than to build on it, and `ROADMAP.md` says so.

---

## 5. What this does not claim

- **lex-sys is not faster than C or Rust today**, anywhere measured. It
  is 1.6× slower (`against-c-and-rust.md`), and the purity benchmark
  agrees at 1.54×.
- **The 158× is one loop**, chosen to make the effect visible. A real
  program with one redundant pure call in a hot loop gets a doubling of
  that call, not of itself.
- **Inlining already wins the small cases.** Everything here is about
  what survives a boundary LLVM will not cross: a large function, a
  separate compilation unit without LTO, recursion. Where inlining
  reaches, C and Rust already have this and pay nothing for it.
- **And lex-sys has no such boundary** (§4.2). The measurement is real,
  the advantage is real, and no program in this language can exhibit it
  until the compilation model changes.
- **`__attribute__((const))` is a fair comparison and a flattering one.**
  It is the strongest form (no memory reads at all); a function reading
  through a shared reference would be `((pure))` in C, which permits less.

---

## 6. Open

| Question | Why it waits |
|---|---|
| Separate compilation | §4.2, and it is now the *precondition* rather than a convenience: without a compilation boundary there is nowhere for a purity fact to be worth anything. `many-files.md` §2.1 lists the three questions it defers — path resolution, cycles, where a library lives — and none has become easier |
| *Pure and cannot trap* | §3. Checkable — a body of `wrapping_*`, comparisons and control flow has no trapping operation — and it is what separates 1.94× from 158×. The awkward part is that it is a property of a *body*, where purity is a property of a signature, so it does not survive a separate compilation the way the row does |
| An LLVM backend that reads it | §4.1. The row already produces `readnone`; this is the first argument for the backend that is about capability rather than speed |
| Purity in the hash | A function's row is already in its `SigId`, so purity is derivable from the hash without the body. Whether a *consumer* should be told is a question about what the content-addressed store promises |
| `__attribute__((const))` emission | If lex-sys ever emits C rather than objects, the row could be written out as the attribute — where it would be, for once, a checked one |

---

## 7. The suite

| Test | Shows |
|---|---|
| `purity_is_the_row_plus_what_a_reference_may_do` | §2's predicate on six cases, including the two a one-condition rule gets wrong |
| `the_three_language_benchmarks_agree` | The four `purity` builds compute the same checksum, which is what makes §4's table a comparison |

| Program | Shows |
|---|---|
| `benches/three/purity.{ls,c,rs}` | §4: the same loop, and the fact only one of the three can state |
| `lex-sys authority --output json` | `"pure"` — which functions the checker proved, and §2.1's count |
