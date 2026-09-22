# What every check costs, and the rule underneath

> **Status: measured, and it falsifies both documents that came before
> it — including the one that was written to correct the first.**
>
> `overflow-cost.md` §3.2 found that a check costs the *vectoriser*
> rather than a branch. It measured the overflow check and wrote "the
> check". `gpu.md` §2.1 caught the over-generalisation, measured the
> **bounds** check, found it free at 1.01×, and replaced the claim with
> a narrower one: what costs the vectoriser is *"the overflow check and,
> as far as anything here has measured, only the overflow check"*.
>
> That is also wrong, and wrong in the same way. Of the **eight** checks
> this language emits inside a loop body, **six** take the SIMD count to
> zero. The overflow check is not special; it was first.
>
> The rule is not about what a check is *about*. §3 is a pair of
> programs with the **same two comparisons and the same trap**, one
> costing **2.16×** and the other **0.99×**, differing only in whether
> the loop already proves the condition. Memory safety is not free.
> Arithmetic is not expensive. **Provable is free, and loaded is not.**

---

## 1. The checks, counted by reading the backend

Every `trapnz`/`trapz` in `crates/lex-sys-codegen/src/lib.rs`, grouped by
what it guards. Eight can appear in a loop body and are what this
document measures; the rest happen once per allocation or per syscall,
where a comparison is not what the time goes on.

| in a loop body | what it tests |
|---|---|
| `a + b`, `a - b`, `a * b` | the operation overflowed |
| `-x` | the one value with no negation |
| `a << b`, `a >> b` | the amount is outside `0..64` |
| `a / b`, `a % b` | **nothing** — see §5 |
| `s[i]` | the index is inside the slice |
| `s[lo..hi]` | `hi > len`, and `lo > hi` |
| `byte_of(n)` | `n` is outside `0..255` |
| `int_of(f)` | `f` is NaN, infinite, or out of range |

| once, not per element | what it tests |
|---|---|
| `box`, `box_slice`, a new arena chunk | `malloc` answered null |
| an arena bump | the chunk cannot spare the bytes, or the size wrapped |
| `alloc_slice` | the count is negative, or `count * stride` overflowed |
| a path to `fs_read`/`fs_write` | length, prefix, `..`, and the prefix boundary |
| `arg(i)` | the index is inside `argc` |

`band`, `bor` and `bxor` emit nothing at all: `bitwise.md` §4 established
they cannot overflow, and the backend says so by having no arm for it.

---

## 2. The measurement

`benches/guards.c` is one kernel per check, each written so the
*unguarded* form is as vectorisable as the instruction set allows —
otherwise the guard would be measured against a loop that was scalar
anyway. `scripts/guards.py` builds each twice, counts the packed
instructions in the emitted `run` with `objdump`, and times both
interleaved. clang 18.1.3, minimum of nine runs.

**Baseline `-O2`** (x86-64, SSE2 — no `-march`):

| check | packed off | packed on | off | on | cost |
|---|---:|---:|---:|---:|---:|
| `s[i]` bounds | 8 | **8** | 62.7 ms | 62.9 ms | **1.00×** |
| `a / b` | 0 | 0 | 536.6 ms | 538.0 ms | **1.00×** |
| `a << b` | 18 | 0 | 133.3 ms | 164.0 ms | 1.23× |
| `a + b` overflow | 8 | 0 | 62.7 ms | 90.4 ms | 1.44× |
| `byte_of(n)` | 11 | 0 | 61.3 ms | 117.2 ms | 1.91× |
| `-x` | 8 | 0 | 63.5 ms | 121.8 ms | 1.92× |
| `s[lo..hi]` | 8 | 0 | 62.2 ms | 134.3 ms | **2.16×** |
| `int_of(f)` | 0 | 0 | 88.9 ms | 222.5 ms | **2.50×** |

**`-march=native`** (this machine has AVX-512), to show the answer is not
an artifact of the instruction set:

| check | packed off | packed on | cost |
|---|---:|---:|---:|
| `s[i]` bounds | 26 | **26** | **1.02×** |
| `a / b` | 0 | 0 | **1.00×** |
| `a << b` | 21 | 0 | 1.15× |
| `a + b` overflow | 26 | 0 | 1.40× |
| `byte_of(n)` | 18 | 0 | 1.55× |
| `-x` | 26 | 0 | 1.82× |
| `s[lo..hi]` | 26 | 0 | **2.14×** |
| `int_of(f)` | 13 | 0 | **3.35×** |

Same ordering, same six zeros, and the float conversion — which had no
SIMD to lose at baseline — becomes the most expensive check in the
language once the hardware can vectorise it.

### 2.1 Two counts, and why this table's 8 is `gpu.md`'s 10

`gpu.md` §2 counted every instruction touching a vector register. That is
the right signal for an integer reduction, where nothing else uses `xmm`,
and the wrong one for a float kernel, where scalar double arithmetic uses
`xmm` too — the guarded `int_of` loop touches *more* vector registers
than the unguarded one while doing no vector work at all.

So `scripts/guards.py` reports both. On the overflow kernel it prints
`10 → 0` by the register count, which is `gpu.md` §2's published figure
exactly, and `8 → 0` by the packed count — the two extra are the `movq`
pair that moves the accumulator between a general register and `xmm`.
The documents agree; the definitions differed.

---

## 3. The control, which is the actual finding

`s[lo..hi]` costs **2.16×** and `s[i]` costs **1.00×**. Both are bounds
checks. Both trap. The subslice does *two* comparisons where indexing
does one, so a first guess is that the count of comparisons is what
matters.

It is not. Kernel 8 is the same subslice check with the bounds taken from
the **induction variable** instead of from memory:

```c
long lo = w[i] & 1;    long lo = i;
long hi = lo + 1;      long hi = i + 1;
TRAP_IF((unsigned long)hi > (unsigned long)n);
TRAP_IF((unsigned long)lo > (unsigned long)hi);
```

| | packed | cost |
|---|---|---:|
| bounds from memory | 8 → **0** | **2.16×** |
| bounds from the induction variable | 8 → **8** | **0.99×** |

Two comparisons either way. The same trap either way. The same
arithmetic either way. The difference is **whether the loop the compiler
already proved bounded also proves these**, and it is the whole
difference between free and the most expensive integer check in the
table.

That is why `s[i]` is free, and it has nothing to do with memory safety:
`i < n` *is* the loop's own condition, so the check is a branch on a
constant, and a branch on a constant is not a branch. Take the same
check off the induction variable — which is exactly what a subslice
does — and it costs more than the overflow trap.

So the axis is not memory-versus-arithmetic, which is what §2.1 of
`gpu.md` implied and this document had to measure to disprove. **The
axis is provability.**

> A check is free when the compiler can discharge its condition from
> what the loop already establishes. It costs reassociation — and with
> it vectorisation — when the condition depends on a value loaded from
> memory, whatever the check is about.

---

## 4. "Vectoriser" is too narrow as well

The baseline `int_of(f)` row has **no packed instructions either way**
and still costs **2.50×**. Nothing was vectorised, so nothing
vectorisable was lost. Reading the two loops says what was:

```
unguarded                          guarded
cvttsd2si (%rsi,%r9,8),%r10        movsd  (%rsi,%rdi,8),%xmm2
cvttsd2si 0x8(%rsi,%r9,8),%r11     ucomisd %xmm0,%xmm2
add    %rax,%r10                   jb     <ud2>
cvttsd2si 0x10(%rsi,%r9,8),%rbx    ucomisd %xmm2,%xmm1
add    %r11,%rbx                   jb     <ud2>
cvttsd2si 0x18(%rsi,%r9,8),%rax    cvttsd2si %xmm2,%r8
add    %r10,%rbx                   add    %r8,%rax
add    %rbx,%rax                   inc    %rdi
add    $0x4,%r9
```

The unguarded loop is unrolled four wide with **four independent
accumulator chains**, which is the same reassociation a vectoriser
needs, spent on scalar instruction-level parallelism instead of lanes.
The guarded loop is one element, one accumulator, and two compares.

So the cost of a trap is not "the vectoriser". It is **reassociation**,
and vectorisation is the largest thing reassociation buys rather than
the only one. A machine with no SIMD at all would still pay for these
checks, which is worth knowing before concluding that a narrower
instruction set makes the guarantee cheaper.

---

## 5. Division is free, for a third reason again

`a / b` costs **1.00×** on both instruction sets, and the backend is why:
`BinOp::Div` lowers to a bare `sdiv` and `BinOp::Rem` to a bare `srem`,
with no comparison emitted at all. Cranelift's `sdiv`/`srem` trap on a
zero divisor and on `int::MIN / -1` because **the hardware does** — an
`idiv` with a zero divisor faults, and the language gets its guarantee
from an instruction that was going to fault anyway.

There is also nothing to lose: integer division has no packed form on
SSE2, AVX2 or AVX-512, so the loop is scalar unguarded and scalar
guarded. The row is `0 → 0` and `1.00×` twice over, and C — where this
same program is undefined behaviour rather than a trap — buys its
undefinedness nothing.

So the three free checks in this language are free for three unrelated
reasons: `s[i]` because the loop proves it, `a / b` because the hardware
does it, and `band`/`bor`/`bxor` because there is nothing to prove.

---

## 6. A constant operand folds the check away

Checked against the compiler rather than assumed. `x << 3` and `x << k`,
from `lex-sys build` and `objdump`:

```
lexs_shift_const:            lexs_shift_var:
    mov  %rdi,%rax               cmp  $0x40,%rsi
    shl  $0x3,%rax               jae  <ud2>
    ret                          mov  %rsi,%rcx
                                 shl  %cl,%rax
                                 ret
```

The constant amount emits **no check at all**. This is §3's rule again at
its easiest setting — a condition on a literal is discharged at compile
time — and it is why the shift check can only be measured with an amount
that comes out of memory.

It also bounds how much of the table a real program pays. `byte_of(65)`,
`s[0..3]` and `x << 8` are free; the same operations on loaded values
are not.

---

## 7. What this means for the thesis

`README.md` calls defined behaviour a design commitment and
`overflow-cost.md` priced one instance of it. The price is larger and
more structural than one bad check:

* **Six of eight** loop-body checks make their operation
  non-reassociable. This is a property of "an operation that may trap on
  a value the compiler cannot bound", not a property of addition.
* The worst is **`int_of(f)` at 3.35×**, not the overflow trap. Nothing
  had looked.
* `gpu.md` §5.1 asks whether **poison** — a per-lane flag reduced at the
  launch boundary — costs less than trapping. That question was about
  one check when it was written. It is now about six, which makes it the
  largest open question in the design rather than a GPU detail.

And it sharpens the roadmap's vectoriser row. An LLVM backend would
vectorise the loops Cranelift leaves scalar — `gpu.md` §2.3 measured
lex-sys at 2.27× off vectorised C *with the trap removed* — but it would
vectorise them only where the traps are not in the way, and §2's table
is the list of places they are.

---

## 8. What this does not say

* **Not that the checks should go.** `defined-behaviour.md` §2.1 is the
  thesis, and a silently wrong answer is what this language exists not
  to give. Knowing the price is not an argument for not paying it.
* **Not that these are lex-sys's numbers.** They are clang's, on kernels
  written in C, because Cranelift emits no SIMD at all (`gpu.md` §2.3) —
  so the current backend pays none of this and gets none of the benefit
  either. The table is what a backend *with* a vectoriser would find,
  which is the one the roadmap wants.
* **Not a claim about every program.** These are loops chosen to
  vectorise. A program whose time goes on calls, branches or cache
  misses pays the `overflow-cost.md` §2 figures instead, which are
  between −9.1% and +3.6%. The shape of the loop decides which table
  applies, and that was §2's finding there and is unchanged.
