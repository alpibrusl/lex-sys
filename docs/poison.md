# Does poison cost less than trapping? Measured

> **Status: measured, and the answer splits exactly where the thesis
> lives.**
>
> `gpu.md` §4.1 proposed **poison** — each lane sets a flag, the
> boundary answers *something went wrong* — as the way this language's
> no-undefined-behaviour claim could survive hardware that cannot stop.
> §5.1 said it had no number and was measurable on a CPU today;
> `check-cost.md` §7 raised it from a question about one check to a
> question about six.
>
> On a wide vector machine, poison is a **large** win for every check
> whose condition is a per-element comparison. Two of them become
> **free**, and the most expensive check in the language drops from
> 3.28× to 1.33×.
>
> On the **overflow check carried by a reduction** — the check the whole
> defined-behaviour argument is named after — poison does not help and
> makes it **worse**, 1.37× to 1.73×. Four kernels separate why, and it
> is not the compiler: "no partial sum overflowed" is a claim about one
> association order, and a vectoriser computes a different one. That
> check is not a per-element property and no spelling makes it into one.
>
> And on a narrow vector ISA the win disappears entirely: at baseline
> `-O2`, poison is worse than trapping in **eleven of twelve** kernels.
> The answer is a property of the target, not of the semantics.

---

## 1. What poison is here

Mode 2 of `benches/guards.c`. Instead of

```c
if (condition) __builtin_trap();
```

the kernel does

```c
bad |= condition;              /* per element */
...
poisoned |= bad;               /* once, after the loop */
if (poisoned) __builtin_trap();
```

and the operation keeps a **defined** result where the check would have
fired: a masked shift, a truncated byte, a wrapped negation, a clamped
conversion. Each of those is one instruction and vectorises, which is
part of what is being priced.

The hypothesis is one sentence. A trap is a side effect at a point, so
the operation cannot be reordered. **A flag OR-ed across elements is a
reduction**, and reductions reassociate — so the loop should vectorise
again.

What it gives up is *where*. `defined-behaviour.md` §2.1 says an
operation with no right answer **stops**; poison says it finishes, and
something later says that it should not have. The program computes with
defined-but-wrong values until the boundary, and the element that caused
it is gone. That is a real weakening and §6 is where it is argued rather
than waved at.

---

## 2. The numbers

`clang 18.1.3`, minimum of nine runs, `scripts/guards.py`. Packed SIMD
counted in the emitted `run` with `objdump`, printed
unchecked / trap / poison.

### 2.1 `-march=native` — this machine has AVX-512

| check | packed | trap | **poison** |
|---|---|---:|---:|
| `s[i]` bounds | 26 / 26 / 26 | 0.95× | **1.00×** |
| `a / b` | 0 / 0 / 0 | 1.01× | **1.01×** |
| `a << b` | 21 / 0 / **53** | 1.17× | **1.00×** |
| `-x` | 26 / 0 / **38** | 1.78× | **0.99×** |
| `byte_of(n)` | 18 / 0 / **46** | 1.61× | **1.23×** |
| `truncate(f)` | 13 / 0 / **59** | 3.28× | **1.33×** |
| `s[lo..hi]` | 26 / 0 / **43** | 2.19× | **1.98×** |
| `a + b` overflow, on a reduction | 26 / 0 / **0** | 1.37× | **1.73×** |

Five of the six checks `check-cost.md` found expensive get most or all
of their cost back, and the SIMD count says why: the loop vectorises
again, harder than it did unchecked, because the flag is extra vector
work the unchecked loop did not have to do.

The exception is the last row, and §3 is about it.

### 2.2 Baseline `-O2` — x86-64, SSE2

| check | packed | trap | **poison** |
|---|---|---:|---:|
| `-x` | 8 / 0 / **24** | 1.96× | **1.45×** |
| `a << b` | 18 / 0 / **48** | 1.27× | 1.64× |
| `byte_of(n)` | 11 / 0 / **38** | 1.79× | 2.18× |
| `s[lo..hi]` | 8 / 0 / **40** | 2.13× | 2.46× |
| `truncate(f)` | 0 / 0 / **58** | 2.47× | 2.79× |
| `a + b` overflow, on a reduction | 8 / 0 / 0 | 1.45× | 1.76× |

**Poison vectorises here too — and is still slower.** The packed counts
go up in every row; the times go up with them. Baseline x86-64 has no
64-bit packed compare, no per-lane variable shift and no packed
double-to-int64, so the vector spelling of each condition is an
emulation several instructions long, and it costs more than the scalar
branch it replaced.

So poison is not a semantic improvement that a compiler cashes in
wherever it can. It is a trade that needs a vector unit wide enough and
complete enough to take it, and on the narrow one it loses.

---

## 3. The one it cannot save, and four kernels to prove it

The overflow check on `total += v[i]` stays at **0 packed** under poison
and gets *worse*. Three controls separate the possible reasons.

| kernel | the condition | packed under poison | poison |
|---|---|---:|---:|
| `overflow` | `total + v[i]`, builtin | **0** | 1.73× |
| `overflow_carried` | `total + v[i]`, sign logic | **0** | 2.02× |
| `overflow_each` | `v[i] + w[i]`, builtin | **0** | 1.04× |
| `overflow_signs` | `v[i] + w[i]`, sign logic | **49** | **0.98×** |

Reading the four rows in order:

**It is partly the spelling.** `__builtin_saddl_overflow` is opaque to
the vectoriser. A signed add overflows exactly when the result differs
in sign from both operands, which is `((a ^ s) & (b ^ s)) < 0` — three
XORs, an AND and a compare, every one of them an operation a vector unit
has. Written that way the element-wise check vectorises fully and is
**free**: 0.98×, 49 packed, against 0 packed for the same semantics
written as the builtin.

That is worth knowing for a language that emits its own IR. lex-sys
lowers `+` to `sadd_overflow` plus `trapnz`; a backend emitting a
wrapping add, the sign test and a flag OR would get the check for
nothing on an element-wise addition.

**And it is partly not the spelling.** `overflow_carried` is the sign
test applied to the reduction, and it does not vectorise either. The
reason is not in any compiler:

> A reduction's overflow condition is a claim about **this** association
> order. Four lanes compute four different partial sums, so "no partial
> sum overflowed" is a different property after vectorising — sometimes
> true where the sequential order overflowed, sometimes false where it
> did not.

A flag cannot fix that, because the flag would be recording the wrong
question. The check is not a per-element property, so poison, whose
whole mechanism is making a check per-element, has nothing to work with.
Paying for the flag and getting no vectorisation is why poison is
*worse* than trapping in that row rather than merely equal.

So the rule, and it is the same shape as `check-cost.md`'s:

> **Poison rescues a check exactly when its condition is a property of
> one element.** A condition the reduction carries is not, and no
> spelling makes it one.

---

## 4. What this answers

`gpu.md` §6 asked one question and it now has three answers, which is
more useful than the one it wanted.

| | answer |
|---|---|
| Does poison cost less than trapping? | **On a wide vector ISA, yes, for every per-element check** — two become free and the worst drops from 3.28× to 1.33× |
| Does it save the *arithmetic* half of the claim? | **Only element-wise.** `a + b` inside a loop is free under poison, written right. `total += a` is not, and cannot be |
| Is it free? | **No.** On baseline x86-64 poison is worse than trapping in eleven of twelve kernels, because the vector spelling of each condition is an emulation |

`gpu.md` §4.1's three options were: wrapping arithmetic in kernels,
poison reduced at the launch boundary, or refusing trapping operations
in kernels. Option (2) now has numbers, and they say it is a **good
trade for element-wise work and no help for a reduction** — which is
awkward, because a reduction is exactly what a GPU is for.

---

## 5. What this does not say

* **Not that lex-sys should switch to poison.** This measures C compiled
  by clang. lex-sys emits Cranelift, which vectorises none of these
  loops (`gpu.md` §2.3), so today the language would pay poison's extra
  work and collect none of its benefit. This is a measurement of what a
  backend *with* a vectoriser would find.
* **Not that the trap is wrong.** `defined-behaviour.md` §2.1 is the
  thesis and a number is not an argument against it. What changes is
  that the thesis now has a priced alternative rather than an assumed
  one, and the alternative is worse exactly where the thesis is loudest.
* **Not a claim about every program.** These are loops chosen to
  vectorise. `overflow-cost.md` §2 measured programs dominated by calls,
  branches and cache, where the whole effect is between −9.1% and +3.6%.

---

## 6. The part that is not a number

Poison trades a stop for a report, and the cost is not in the table.

A trap names the operation: the process dies at the instruction that had
no answer, and a debugger has the element in a register. Poison says
*somewhere in this loop*, after the loop has finished writing its output
— and the wrong values have been used in the meantime, because the
fallback (`& 63`, `& 255`, a clamp, a wrap) is defined and silently
wrong. `strings.md` §2 refuses truncation on exactly that ground, and
`line-reading.md` found `cut` printing 60 KB of the wrong field with
exit 0, which is what a silently wrong answer looks like when it reaches
a user.

So poison is not a cheaper trap. It is a **different failure model**,
and adopting it would be a change to `defined-behaviour.md` §2.1 rather
than an optimisation underneath it. The honest framing for a future
decision:

* Where a program cannot act on *which* element failed — a kernel whose
  whole output is discarded on failure — poison loses nothing it was
  using, and §2.1's table is what it buys.
* Where a program reports, recovers, or debugs, it loses the thing that
  made the guarantee worth having.

That is a language decision with a measured price on one side and an
unmeasured one on the other, which is the right state for it to be in
before anyone builds it.

---

## 7. Open

| Question | Why it waits |
|---|---|
| Does the sign spelling pay off in Cranelift? | §3 found the builtin opaque to *clang*. lex-sys emits `sadd_overflow` + `trapnz` and Cranelift vectorises nothing, so the spelling is free to change and currently buys nothing. It becomes a real question the day the backend has a vectoriser |
| A per-element trap that is not a stop | The middle option nobody has costed: a trap that records the element and continues, which is poison with the `where` kept. It costs a store per failure and nothing per success, and no kernel here measures it |
| The reduction case, properly | §3 says "no partial sum overflowed" is not reassociable. The question that leaves open is whether a *different* claim — no overflow in any association order, which is a bound on the inputs — is cheap enough to be worth having, and that is arithmetic rather than measurement |
