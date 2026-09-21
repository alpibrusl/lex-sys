# Against C and Rust

> **Status: measured, and the answer is 1.6×.**
>
> `docs/overflow-cost.md` §4 named this as a measurement the repository
> owed and had not made: *"It does not say the language is 40% slower
> than C. […] The comparison between lex-sys and C at equal semantics is
> a different measurement, and this repository has not made it."*
>
> It is made now. The headline is that **at equal semantics lex-sys is
> about 1.6× C and Rust**, that Rust and C are within 4% of each other,
> and that the gap is therefore a backend gap rather than a language one.

---

## 1. The discipline, because it is most of the work

Three rules, and each rules out a comparison that would have been easier
and worthless.

**The same algorithm, line for line — not the same task.** Comparing
`examples/sort` to GNU `sort` would measure decades of tuning and an
external merge sort. `benches/three/mandelbrot.{ls,c,rs}` are the same
loop written three times, with the same variables carried across
iterations in the same order.

**The same semantics.** Rust's release profile *wraps* on overflow;
lex-sys traps. Reporting `rustc -O` against lex-sys would be reporting a
semantic difference as a performance difference, so Rust is built both
ways and the honest row is the one that also traps.

**The same answer.** Every build prints a checksum, and the harness
refuses to report a time when they differ. That is not ceremony: an early
draft had a fixed-point constant wrong in one language and the numbers
looked fine.

```sh
$ python3 scripts/three.py
```

---

## 2. The numbers

linux-x86_64, Xeon @ 2.10GHz, minimum of seven runs, `cc -O2` (gcc 13)
and `rustc -O` (1.98).

### Compute-bound: Mandelbrot, Q16.16 fixed point

| build | time | vs C at equal semantics |
|---|---|---|
| **lex-sys** (traps) | 0.2098s | **1.69×** |
| C `-O2` (traps) | 0.1240s | 1.00× |
| Rust `-O` (traps) | 0.1288s | 1.04× |
| C `-O2` (wraps) | 0.1011s | 0.82× |
| Rust `-O` (wraps) | 0.1139s | 0.92× |

All five compute 39,690,297.

### Memory-bound: sieve of Eratosthenes

| build | time | vs C |
|---|---|---|
| **lex-sys** | 0.3189s | **1.56×** |
| C `-O2` | 0.2044s | 1.00× |
| Rust `-O` | 0.1633s | 0.80× |

All three compute 6057.

---

## 3. What the numbers say

**1.6×, on both kernels, is the number to quote.** Not 1.0, and not the
40% figure from `overflow-cost.md` — that one measured trapping against
wrapping *within* lex-sys, which is a different question and is why this
document exists.

**Rust is within 4% of C at equal semantics, and lex-sys is not.** That
is the load-bearing observation. Rust carries linear-ish ownership, a
borrow checker, bounds checks and monomorphisation, and pays essentially
nothing for them on this kernel. So the 1.6× is not the price of safety,
ownership, or effect rows — those are erased at compile time here exactly
as the README claims. It is **Cranelift against LLVM**, and it is the
concrete version of the README's *"any larger gap early on is
implementation maturity, not language design."*

That claim was an assertion when it was written. It now has a number
attached and a falsifier: if an LLVM backend lands and the gap stays at
1.6×, the claim was wrong.

**Rust beat C on the sieve, which is a warning about this whole
document.** 0.80× is not a language result; it is two implementations of
one algorithm meeting different optimisers. The mandelbrot triple is the
stronger evidence because the three sources are line-by-line
translations with identical data layout, and the sieve pair is not quite
that — Rust's clearing loop is an iterator that LLVM turns into a
`memset` and the C is a written-out loop. **Treat the sieve row as a
second opinion rather than a second measurement.**

**And the measurement floor is higher than it looks.** The same lex-sys
sieve, compiled from a version that returns its answer rather than
printing it, ran 15% faster — a difference made entirely of code layout,
the effect `overflow-cost.md` §3.3 found when padding an object moved a
loop by a few bytes. Anything here under about 15% is not a result.

---

## 4. What the absence of `float` costs

This is the part that is not about speed.

lex-sys has no floating point (`reach.md` §2), so the Mandelbrot above is
Q16.16 fixed point: a value is the real number times 65536, a product is
`(a * b) >> 16`. The C and Rust versions were written the same way so the
comparison would be fair — but a program that actually wanted this
picture would have written `double`.

So here is that program, as the row lex-sys cannot enter:

| build | time | vs C fixed-point |
|---|---|---|
| C `-O2` f64 | 0.1449s | 1.17× |
| Rust `-O` f64 | 0.1454s | 1.17× |

### 4.1 The surprise: f64 is *slower* here

Double precision is **17% slower than Q16.16 integer arithmetic** on this
kernel, in both C and Rust, while doing slightly *less* work (the f64
runs total 39,684,084 iterations against fixed point's 39,690,297).

Integer multiply and shift have better throughput on this processor than
double multiply, and the kernel is three multiplies and four adds per
iteration with no transcendental anywhere. So on this shape of work,
**not having floats is not a speed problem.**

That is worth stating plainly because the intuition runs the other way,
and because it narrows what the missing feature actually costs.

### 4.2 What it does cost: fifteen decimal digits

| | resolution |
|---|---|
| Q16.16 | 1.53 × 10⁻⁵ |
| f64 | 2.22 × 10⁻¹⁶ |

Ten orders of magnitude, and the picture shows it: the two runs disagree
by 6,213 iterations out of 39.7 million, **0.0157%**. Pixels near the
boundary land on the wrong side because the grid coordinate itself is
only accurate to 1.5 × 10⁻⁵.

For a benchmark that is nothing. For numerical work it is the whole
subject:

- **Deep zooms are impossible.** Q16.16 holds values in ±32768 with
  15 fractional bits. A Mandelbrot zoom past about 10⁴ has no bits left.
- **Range and precision trade against each other, manually.** Q16.16,
  Q32.32 and Q48.16 are three different types in a language with one
  integer, and choosing between them is the programmer's arithmetic,
  checked by nothing. A `float` carries its exponent.
- **Every standard numerical method assumes it.** Condition numbers,
  convergence criteria, error bounds — the literature is written in
  floating point, and a fixed-point port of any of it is a new piece of
  analysis rather than a translation.

**So the honest summary is that `float` is an expressiveness gap, not a
performance gap**, and `reach.md` §6's row — *"no design question that
is known"* — still holds. What this measurement adds is that adding it
should be justified by what programs become writable, and not by a
promise that they will get faster, because on evidence they will not.

---

## 5. What this does not measure

- **Two kernels.** Compute-bound and memory-bound, one each. Not a suite,
  and not startup time, code size, compile time or anything allocating in
  a loop.
- **One machine, one afternoon**, on the compilers that happened to be
  installed. `scripts/three.py` is committed so the table can be
  re-measured rather than believed.
- **Nothing about safety.** Every one of these programs is correct in all
  three languages. The interesting comparison — what it costs to write
  the *wrong* program — does not fit in a benchmark.
- **No I/O.** `examples/base64` and `examples/sort` would have made
  better-looking programs to compare, and the comparison would have been
  against three different buffering strategies.

---

## 6. Open

| Question | Why it waits |
|---|---|
| ~~Anything lex-sys does *better*~~ | **Answered — `docs/purity.md`.** There is exactly one candidate and it is structural: the row is a checked purity proof, which C can only promise and Rust cannot state. Measured at 1.94× (CSE) and 158× (hoisting a loop-invariant call), on 35% of the functions here, and collected by nothing today |
| An LLVM backend | §3 makes this falsifiable: the claim is that 1.6× is Cranelift. The README already commits to "Cranelift for dev, LLVM for release", and this is the number that says what the second half is worth |
| `jo` instead of `seto`/`test`/`jne` | `overflow-cost.md` §3.4. Three instructions for one on every checked operation, and this kernel does seven per iteration — the cheapest place to look for part of the 1.6× |
| `float` | §4. Now with an argument attached: expressiveness, not speed, and the design question is which of IEEE-754's corners (NaN ordering, reassociation, `-0.0`) this language defines rather than inherits |
| A third kernel that allocates | §5. Both of these run in a fixed footprint, so nothing here tests `heap.md`'s allocator against `malloc` or Rust's |

---

## 7. The suite

| Program | Measures |
|---|---|
| `benches/three/mandelbrot.{ls,c,rs}` | Compute-bound, identical line by line, five builds, one checksum |
| `benches/three/mandelbrot_f64.{c,rs}` | §4: the row lex-sys cannot enter |
| `benches/three/sieve.{ls,c,rs}` | Memory-bound, and §3's warning about reading it too hard |
| `scripts/three.py` | Builds all of them, refuses to time builds that disagree, prints the tables above |

| Test | Shows |
|---|---|
| `the_three_language_benchmarks_agree` | Every build still computes the same checksum, which is what makes the timing meaningful. Not a timing gate — wall-clock in CI is noise |
