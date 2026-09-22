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
bitcast. A correctly-rounded square root is *not* expressible here,
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
| `sin`, `exp`, `log`, `pow` | §4. Library code with a stated accuracy, and nothing has asked. When something does, the shape is `std.fmt`'s: written in lex-sys, checked against an oracle |
| Float `abs`, `min`, `max` | §4. One asker with a working alternative |
| A total order | `floating-point.md` §7's other row, untouched here |
| `sqrt` of a negative | Answers NaN, which is what the instruction does and what IEEE-754 says. Not a trap: `floating-point.md` §2.1 already settled that NaN announces the absence of a value rather than lying about one, and a square root of −1 is exactly that case |
