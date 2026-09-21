# What the overflow trap costs

> **Status: measured, and the README was wrong.**
>
> Not about the number — the number is right about as often as it is
> wrong. About the **reason**. `README.md` said the cost was "a low
> single-digit percent" and `defined-behaviour.md` §2.1 said why: *"a
> branch that is never taken is close to free."* The branch is close to
> free. The branch is not the cost.
>
> This document exists because an outside reader said the claim should be
> measured before it was sold, and they were right: nineteen slices in,
> this repository had no benchmark of any kind.

---

## 1. What was measured

Four programs, each written twice. The `_checked` half uses `+`, `-` and
`*`, which trap on overflow. The `_wrapping` half is the same program with
`wrapping_add`/`wrapping_sub`/`wrapping_mul`, which lower to a bare
`iadd`/`isub`/`imul` (`crates/lex-sys-codegen`). Nothing else differs, so
the gap between the two binaries is the guarantee and nothing else.

```sh
$ python3 scripts/bench.py --with-c
```

Interleaved A/B, minimum of nine runs per half, linux-x86_64, Cranelift at
`opt_level = "speed"`:

| benchmark | what dominates | checked | wrapping | difference |
|---|---|---|---|---|
| `sum` | arithmetic, nothing else | 0.3007s | 0.2141s | **+40.5%** |
| `sieve` | strided writes, cache | 0.2812s | 0.2715s | **+3.6%** |
| `scan` | comparisons, branches | 0.2541s | 0.2794s | **−9.1%** |
| `fib` | calls and returns | 0.0145s | 0.0141s | **+2.8%** |

Three of the four are inside "a low single-digit percent" — one of them
because the checked build was *faster*. The fourth is forty percent.

---

## 2. So the claim is not wrong, it is the wrong shape

> **The cost of checked arithmetic is not a percentage. It is whether
> arithmetic is on the critical path.**

Which is obvious once it is written down, and is not what either document
said. `README.md` gave a single number, as though the check were a tax on
a program's size. It is a tax on one resource, and most systems code is
waiting for a different one — a cache line, a branch, a syscall.

The honest summary is a range with a rule attached, and both documents now
carry it.

---

## 3. Why the worst case is forty percent, which is not the branch

### 3.1 The branch really is close to free

`fib` runs three checked operations per node and pays 2.8%. `sieve` runs a
checked add in its innermost loop and pays 3.6%. If a never-taken branch
were expensive, those would not be the numbers.

### 3.2 The cost is that a trap is observable

Here is the same reduction in C, compiled at `-O2`, once with
`__builtin_saddl_overflow` and `__builtin_trap` and once with plain
wrapping addition (`benches/reduce.c`):

| compiler | checked | wrapping | difference |
|---|---|---|---|
| clang 18 | 0.0929s | 0.0636s | **+46.1%** |
| gcc 13 | 0.1532s | 0.0881s | **+73.8%** |

A mature optimising backend is *worse* at this than Cranelift, which
settles what is going on. Counting the SIMD instructions in clang's inner
loop:

```
unchecked    26
checked       0
```

**The check does not cost a branch. It costs the vectoriser.** An addition
that may trap is an addition whose order is observable, so it cannot be
reassociated, so a reduction cannot be split across lanes. The unchecked
loop adds four at a time; the checked loop adds one at a time and then
also tests a flag.

That is a real, structural cost, it is exactly the one the README
promised there would not be, and it lands precisely where the README's own
"linearity gives the optimiser stronger aliasing facts" argument wants to
collect its winnings — the hot arithmetic loop.

### 3.3 And the benchmark that came out backwards

`scan` is 9% **faster** checked than wrapping, consistently, across four
different code layouts (padding the object with unused functions to move
`words` in memory changed the figure between −5.8% and −10.5% and never
changed its sign).

We do not have an explanation, and this document is not going to invent
one. What can be said is bounded and worth saying: the difference is about
0.6 cycles per iteration in a loop whose real work is a load, four
compares and two unpredictable branches, and at that scale the things that
decide the time are not in the source language. It is evidence for §2 —
where arithmetic is not the bottleneck, removing the check does not
reliably make a program faster, and may not make it faster at all.

### 3.4 One thing that is ours to fix

Cranelift lowers `sadd_overflow` + `trapnz` to three instructions where
one would do:

```
add    $0x1,%rdx
seto   %r10b          # ... into a register
test   %r10b,%r10b    # ... then test the register
jne    <ud2>          # instead of `jo <ud2>`
```

That is not the vectoriser problem and it is not a language cost — it is a
lowering that has not been taught the idiom. It will not rescue `sum`,
whose ceiling is set by §3.2, but it is real and it is in the part of the
stack this project controls. Filed in §5.

---

## 4. What this does not say

- **It does not say trapping is wrong.** `defined-behaviour.md` §2.1's
  argument is unchanged: a silently wrong answer propagates and a stopped
  process does not, and nothing measured here bears on that. A cost being
  larger than advertised is a reason to advertise it correctly, not a
  reason to stop paying it.
- **It does not say the language is 40% slower than C.** It says an
  arithmetic-bound reduction is, in both lex-sys and C, when the arithmetic
  is checked. The comparison between lex-sys and C at equal semantics is a
  different measurement, and this repository has not made it.
- **It is one machine and one afternoon.** Four programs on
  linux-x86_64, no aarch64 numbers, no profile beyond wall-clock. The
  script is committed so the next person does not have to trust the table.

---

## 5. Open

| Question | Why it waits |
|---|---|
| `jo` instead of `seto`/`test`/`jne` | §3.4. Ours to fix, in the sense that it is a Cranelift lowering we could contribute or work around. Worth measuring against `sieve` and `scan`, where the instruction count is a larger share of the loop than in `sum` |
| The aarch64 numbers | CI runs darwin-aarch64 and this table does not. Flag-setting and branch behaviour differ enough that the numbers are not transferable |
| `@wrapping` on a block or a function | If a hot loop wants the bits, it currently says so once per operation. A scoped form would keep the default honest and the loop readable — and would need to be impossible to reach by accident, which is the hard part |
| Checked arithmetic against real C | §4's second bullet. Needs a program that exists in both languages, which is the port `reach.md` §6 also wants |

---

## 6. The suite

| Test | Shows |
|---|---|
| `every_benchmark_pair_agrees` | Both halves of every pair compile and compute the same answer, which is what makes the timing meaningful. Not a timing gate — wall-clock in CI is noise |
| `scripts/bench.py` | The table above, re-measured rather than quoted |

| Program | Measures |
|---|---|
| `benches/sum_*.ls` | The ceiling: arithmetic and nothing else |
| `benches/sieve_*.ls` | Memory-bound |
| `benches/scan_*.ls` | Branch-bound, and §3.3 |
| `benches/fib_*.ls` | Call-bound |
| `benches/reduce.c` | §3.2: what a mature backend does with the same guarantee |
