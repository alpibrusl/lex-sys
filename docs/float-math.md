# `sqrt`, and the capability question it was waiting on

> **Status: settled and built, and the measurement is the argument.**
>
> `floating-point.md` §7 has carried this row since `float` landed:
>
> > *`std.math` over floats — `sqrt`, `sin`, `exp`. Each is either a libc
> > call, gated by `Ffi`, which would make arithmetic need a capability,
> > or an implementation with its own error analysis. **The capability
> > question has to be settled first.***
>
> The capability question turns out to be **the wrong question for
> `sqrt`**, and answering the right one took measuring what the programs
> that hand-rolled it actually compute. One of them is wrong by 143
> orders of magnitude.

---

## 1. Two programs asked, by writing it themselves

`line-reading.md` §1 set the rule and then enforced it against a library
function that had one asker. This one has two, counted the same way — by
reading them:

| | what it hand-rolls | why |
|---|---|---|
| `examples/newton.ls` | a square root by Newton's method, five steps | the program *is* the demonstration |
| `benches/game/spectral.ls` | `sqrt_of`, twenty steps | spectral-norm needs one, and the source says so |

`spectral.ls` names the blocker in its own header: *"`sqrt` is written
here because `floating-point.md` §7 leaves `std.math` over floats open —
the capability question comes first."*

Two askers, two *different* algorithms, and neither is right.

---

## 2. Both hand-rolled roots are wrong, and one is spectacularly wrong

`sqrt_of` against a correctly-rounded square root, over 40,008 values —
the specials, twenty thousand uniform in 10⁻⁶…10⁶, and twenty thousand
spread across the exponent range:

| | |
|---|---|
| not correctly rounded | **23,362 — 58.4%** |
| worst error | **2.15 × 10¹⁸ ulp** |

That last number is not a rounding error. It is a different answer:

| x | `sqrt_of(x)` | correct |
|---|---|---|
| 10¹⁰⁰ | 4.768372 × 10⁹³ | 10⁵⁰ |
| 10²⁰⁰ | 4.768372 × 10¹⁹³ | 10¹⁰⁰ |
| 10³⁰⁰ | 4.768372 × 10²⁹³ | **10¹⁵⁰** |

The first guess is `x / 2`, and a Newton step roughly halves the distance
to the root, so twenty steps move 5 × 10²⁹⁹ down by a factor of 2²⁰ and
stop — 143 orders of magnitude short. The function does not converge and
does not say so.

Its comment reads: *"Twenty is past the point where binary64 stops
changing."* That is true of the inputs spectral-norm feeds it, which sit
near 1.27, and false of the function, which takes any `float` and
returns without complaint.

**This is `cut`'s long line again** (`line-reading.md` §2): a shipped
program, checked against a published expected output, correct on every
input anyone tried, and wrong on the dimension the test never varied.
The benchmark's answer is right — `1.274219991` — because it never asks
for a root outside a narrow band. The *function* is wrong.

`newton.ls` is better and still not correct: five steps from a fixed
guess of 2.0 gives √2 to **1 ulp**. That is fine for what it
demonstrates — it prints residuals and is about convergence — and it is
not a square root anyone should call.

---

## 3. So the capability question was the wrong question

§7's row assumed two options: a libc call gated by `Ffi`, or an
implementation with its own error analysis. **Neither is what `sqrt`
is.**

`sqrt` is **one instruction**: `sqrtsd` on x86-64, `fsqrt` on
aarch64. Cranelift emits it directly. So:

* It reaches **no library**, so there is no `Ffi` to gate, so
  arithmetic does not acquire a capability. The fear in §7's row was
  real and does not apply here.
* Its row is `[]` and `lex-sys authority` reports nothing, which is
  correct: a square root observes nothing outside the program.

And the error analysis option is closed by §2 rather than by taste:
**IEEE-754 requires `sqrt` to be correctly rounded**, the instruction
is, and no sequence of `+`, `-`, `*` and `/` in lex-sys reliably is —
which the 58.4% measures.

### 3.1 Which is why this one is a builtin and printing is not

`float-printing.md` made the opposite call for the same-shaped question,
and both calls are right for the same reason: **put it where it can be
correct.**

Printing a float is a *decision procedure* — Steele and White over exact
integers — and lex-sys can express it, so `std.fmt.float_into` is
library code and the compiler's whole contribution is `bits_of`, a
bitcast (plus a select that gives every NaN one pattern,
[`differential.md`](differential.md) §4). A correctly-rounded square root is *not* expressible here,
because the only correct implementation is an instruction. One goes in
the library because it can; the other goes in the compiler because it
cannot.

---

## 4. What is not in this slice

**Not `sin`, `exp`, `log` or `pow`.** Those are the half of §7's row
that is genuinely about error analysis: none is a single instruction,
each is a routine with an argument reduction and a polynomial, and each
would be library code with a stated accuracy. **No program here has
asked for one**, and §1's rule is the rule.

> **Corrected (§7 below).** `exp`, `log` and `pow` are now built, once
> three programs asked for them by name. `sin` stays open — nothing has
> asked, and it needs its own range reduction, not the one the other
> three share.

**Not float `abs`, `min` or `max`.** One asker between them —
`newton.ls`'s `magnitude` — and it has an alternative that works:
`if x < 0.0 { return -x; }` is three lines and correct.

That is the same test `line-reading.md` §4 used to *admit*
`buffer.clear` with one asker: the question is not how many programs
want it but whether the ones that want it have a working alternative.
`clear` had none — a buffer could not be reused at all. `magnitude` has
one, so it stays in the program that needs it.

**Not a `std.math` module for floats at all.** `sqrt` is a builtin, so
there is nothing to import and nothing to put in one. `std.math` stays
integer-only until something earns a place beside it.

> **Corrected (§7).** `exp`, `log` and `pow` earned that place, and
> `std.math` is where they went — the same module `sqrt` itself never
> needed to join.

---

## 5. What it fixed

| | before | after |
|---|---|---|
| `benches/game/spectral.ls` | its own 20-step Newton, 58.4% not correctly rounded | `sqrt`, and the benchmark still answers `1.274219991` |
| `examples/newton.ls` | keeps its Newton loop — **on purpose**, it is the demonstration | prints `sqrt`'s answer beside its own, so the program now shows what five steps are worth |

`sqrt_agrees_with_the_hardware` checks the builtin against Rust's own
`f64::sqrt` over the same 40,008 values, including the four exponents
where `sqrt_of` was off by 10⁴³ and more.

---

## 6. Open

| Question | Why it waits |
|---|---|
| ~~`exp`, `log`, `pow`~~ | **Built** — §7 |
| `sin` (and `cos`) | Still nothing has asked, and unlike `exp`/`log`/`pow` it needs its own range reduction (mod 2π, which loses precision by subtraction for large arguments in a way none of the other three does) rather than sharing theirs |
| Float `abs`, `min`, `max` | §4. One asker with a working alternative |
| A total order | `floating-point.md` §7's other row, untouched here |
| `sqrt` of a negative | Answers NaN, which is what the instruction does and what IEEE-754 says. Not a trap: `floating-point.md` §2.1 already settled that NaN announces the absence of a value rather than lying about one, and a square root of −1 is exactly that case |

---

## 7. `exp`, `log` and `pow`, closed the way §6 said they would be

Three askers, the two-per-half bar §1 already used, each wanting more
than one of the three: `examples/growth.ls` (continuous and discrete
compound growth, plus a doubling time — `exp`, `pow` and `log` in one
program), `examples/decay.ls` (a half-life table, computed the
differential-equation way and the definitional way side by side — `exp`
and `pow` again, cross-checked against each other), `examples/entropy.ls`
(Shannon entropy of standard input's byte distribution — `log`, the
third caller). Between them: `log` three askers, `exp` and `pow` two
each.

**Library code with a stated accuracy**, exactly as §4 said it would be,
in `std/math.ls`:

* `exp(x)`: range reduction to `x = k*ln2 + r` with `|r| <= ln2/2` (`ln2`
  split into a high part and a low residual, the standard technique, so
  `k*ln2_hi` loses no precision for the `k` this produces), a 14-term
  Taylor series for `e^r`, and `pow2(k)` — 2^k by exponentiation by
  squaring on ordinary multiplication — to rescale.
* `log(x)`: pull the unbiased binary exponent `e` out of `x`'s own bits
  with `bits_of` so `x / pow2(e)` is a mantissa `m` in `[1, 2)`, then a
  14-term series in `y = (m-1)/(m+1)`.
* `pow(x, y)`: `exp(y * log(x))` for `x > 0`, which is most of what a
  caller wants it for; `x <= 0` gets its own cases, matching the two
  conventions C's `pow` already settled rather than reinventing them.

None of the three needs `bits_of`'s missing other half — a builtin that
builds a `float` back up from bits, which does not exist. Scaling by an
integer power of two is exact under ordinary multiplication as long as
it does not overflow, so `pow2` gets there by squaring rather than by
bit construction. The one place that bit missing, if it existed, would
have simplified something: `exp`'s own scaling still had to split its
exponent in half before multiplying (below), where a direct `ldexp`
would not have.

**Measured, not asserted**: `crates/lex-sys/tests/conformance/floats.rs`
checks all three against Rust's own `f64::exp`/`f64::ln`/`f64::powf`
over roughly 4,500 generated values, plus specials, within **1e-9
relative error** — two orders of magnitude looser than what was actually
measured while writing this (2.4e-14 worst case for `exp`, 6e-14 for
`log`, away from where relative error stops meaning anything, the same
caveat §2 already states for `sqrt`'s own tails). **Not correctly
rounded, and not claimed to be** — §2 already found that unreachable for
a hand-rolled `sqrt`, and nothing about `exp`/`log`/`pow` makes it more
reachable.

### 7.1 The bug the differential test found

The first version of `exp` answered **infinity for `exp(709.5)`**, which
is finite (≈1.3549863 × 10³⁰⁸, comfortably under `f64::MAX`). The cause
was `pow2(k)` computed as one call: at `x = 709.5`, `k = 1024`, and
`2^1024` alone overflows a `float` even though `sum * 2^1024` (`sum`
always sitting in `[0.5, 2)`) would not have. Splitting the exponent —
`sum * pow2(k - k/2) * pow2(k/2)` — keeps every intermediate value in
range up to the true overflow point (≈709.7827) and reaches infinity
correctly exactly there, through ordinary IEEE overflow rather than a
guard. `log`'s own `pow2(e)` call never hits this: a normal `float`'s
unbiased exponent never reaches 1024, only an infinite input's raw bits
do, and that is caught earlier by an explicit check.

The other two guards each answer a different hazard than the arithmetic
does: `is_nan(x)` in both `exp` and `log`, because `truncate` — which
both use internally, to round to the nearest integer `k` or to check
whether `y` is a whole number in `pow` — traps on NaN
(`docs/floating-point.md` §4), and a library function should not trap on
an input its own domain does not exclude. `exp`'s `|x| > 750` guard and
`log`'s `x > f64::MAX` guard exist for the same reason, one step further
out: an infinite `x` would also send `x / ln2` or `x / pow2(e)` somewhere
`truncate` traps on.
