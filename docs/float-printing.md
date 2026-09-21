# Printing a float

> **Status: settled and built.**
>
> `floating-point.md` §7 listed printing as *"the one that makes `float`
> awkward rather than incomplete"*, and that was the right word. A
> language with `float` and no way to print one reports numbers through
> `truncate` and a scale factor, which is fixed point with extra steps —
> `examples/newton.ls` did exactly that, and said so.
>
> This document is that row closed. The answer is `std.fmt.float_into`,
> and the thing worth reading it for is **where** the answer lives:
> not in the compiler.

---

## 1. What "printing a float" means

There are three things a printer could aim at, and only one of them is
a contract:

| Aim | Example for `0.1` | Verdict |
|---|---|---|
| The exact value | `0.1000000000000000055511151231257827` | True, and useless. Nobody typed that |
| Some fixed number of digits | `0.100000` | Loses values. `1e300` and `1e-300` both print as `0.000000` |
| The **shortest decimal that reads back to the same bits** | `1e-1` | The only one that is a round trip |

The third is the contract, and it is the one this implements: for every
`float` `x`, reading back what `float_into` wrote gives `x` again, and no
shorter decimal would have.

That property is worth stating precisely, because it is stronger than
"accurate". `0.1000000000000000055511151231257827` is more accurate and
still wrong for this purpose: the value it names is the same `float`, so
the extra digits carry no information and cost thirty characters.

### 1.1 The form

```
d[.ddd]e[-]k
```

One digit, optionally a point and the rest, then `e` and a decimal
exponent. `0.1` is `1e-1`, `1.5` is `1.5e0`, `100.0` is `1e2`, zero is
`0e0`.

This is not the prettiest choice and it is deliberate. Positional
notation — printing `0.1` as `0.1` and `1e21` as `1000000000000000000000`
— needs a *second* set of decisions (at which exponent does it switch?
how many leading zeros before it gives up?) which have nothing to do with
the digits and which every language answers differently. Those decisions
are a formatting layer's business. The digits are the hard part, and this
is the layer that gets them right.

`inf`, `-inf` and `NaN` are spelled rather than refused, because
`floating-point.md` §2 made them values. `-0.0` prints as `-0e0`: the
sign bit is part of the value, and losing it on the way out would make
printing the one place the language forgot.

---

## 2. Where it lives, which is the point

`float_into` is **written in lex-sys**, in `std/fmt.ls`. It is not a
builtin, not a call into libc, and not a special case in the code
generator. The compiler's entire contribution is one instruction:

```
bits_of(x: float) -> int          // the same 64 bits, read as an integer
```

A bitcast. From there the algorithm is ordinary lex-sys: masks and
shifts to pull the mantissa and exponent apart (`bitwise.md`), a
`region` to hold the working numbers, `while` loops, and a `[byte]` to
write into. Its effect row is `[]` — printing a float performs nothing,
because writing the *bytes somewhere* is the caller's effect, not this
function's.

This matters more than it looks. The usual arrangement is that a
language's float printing is a thousand lines of C or Rust inside the
runtime, reachable only through whatever interface it chose to expose.
Here the worst piece of numerical formatting there is turned out to be
*library code in the language itself*, and the thing that made it
possible is a builtin small enough to describe in six words.

It is also the honest test of `bits_of`. A primitive that only the
compiler's own code could use would not have earned its place;
`float_into` is the program that proves a user could have written this.

---

## 3. The algorithm

Steele and White's, which Dragon4 is a refinement of and Grisu and Ryū
are the fast approximations to.

Keep the value as an exact ratio of integers `R / S`, and keep alongside
it the distance to the midpoint of each neighbouring float, `M+` and
`M-`. Scale until the value sits in `[0.1, 1)`. Then, repeatedly:
multiply `R` by ten, take the digit, and ask whether what remains is
already closer to this float than to either neighbour. When it is, stop —
the digits emitted so far are the shortest that round-trip.

The stopping rule is the whole algorithm. It is why the output is
shortest rather than merely correct, and it is what Grisu2 cannot
guarantee and falls back to Dragon4 for.

The integers involved reach **1080 bits** (§4), so `std.bignum` is
underneath — also written in lex-sys, also in this repository, and
ninety lines rather than four hundred for the reason in §3 below.

### 3.1 The factors of two, and why the setup has four cases

A `float` is `m * 2^e` exactly. To make `R`, `S`, `M+` and `M-` all
integers, everything is scaled by a power of two:

| | `R` | `S` | `M-` | `M+` |
|---|---|---|---|---|
| `e >= 0`, even neighbours | `m * 2^(e+1)` | `2` | `2^e` | `2^e` |
| `e >= 0`, uneven | `m * 2^(e+2)` | `4` | `2^e` | `2^(e+1)` |
| `e < 0`, even neighbours | `m * 2` | `2^(1-e)` | `1` | `1` |
| `e < 0`, uneven | `m * 4` | `2^(2-e)` | `1` | `2` |

"Uneven" is the case every paper on this spends a paragraph on: at a
power of two the gap to the float *below* is half the gap to the float
*above*, because the mantissa just rolled over. A printer that ignores
it is wrong on exactly the values a test suite is most likely to
contain — `1.0`, `0.5`, `2.0`, every `2^k` — which is why the corpus in
§5 contains all 2046 of them rather than a sample.

### 3.2 No division, which is why `std.bignum` is ninety lines

A digit is `R / S` for two bignums. That sounds like it needs bignum
division, which is the single nastiest routine in an arbitrary-precision
library — Knuth 4.3.1 algorithm D, normalisation, quotient estimation,
the correction step.

It does not, because **the digit is 0 to 9**. Nine subtractions settle
it:

```
var digit = 0;
while bignum.compare(r, s) >= 0 {
    bignum.subtract(r, s);
    digit = digit + 1;
}
```

`std.bignum` therefore has no division at all. It has `compare`,
`subtract`, `add_into`, `mul_small`, `shift_left`, `copy` and `set`, all
in-place over a fixed-length `[int]` of base-2³² limbs, all
allocation-free — which is what lets the caller keep five of them in one
`region` and never touch the heap.

Base 2³² rather than 2⁶³ for one reason: a limb has to survive being
multiplied by ten. `10 * (2^32 - 1)` is about 2^35.7 and fits; the same
product in base 2⁶³ would trap on overflow.

### 3.3 The boundaries are inclusive for an even mantissa — a measured digit

Reading a decimal back uses round-to-nearest-**even**. So when the
mantissa is even, a decimal sitting exactly on the midpoint between this
float and its neighbour still reads back to *this* float: the tie goes to
the even one, which is this one. The boundary is inclusive.

That turns four comparisons from `<` into `<=`, and it is not cosmetic.
Without it, `5.299064834871378e16` printed as
`5.2990648348713776e16` — seventeen digits where sixteen round-trip.
Correct, and one digit too long, which is precisely the failure the
shortest-printing contract exists to rule out. Two of the first 4000
corpus values caught it.

### 3.4 A dead-level tie, where two answers are both right

Once in a while the exact value sits *exactly* halfway between two
shortest decimals. `2^-25` is one: its exact value is

```
0.0000000298023223876953125
```

and truncating to seventeen significant digits leaves exactly `5` with
nothing after it. Both `2.9802322387695312e-8` and
`2.9802322387695313e-8` are seventeen digits, and both read back to
`2^-25`. Neither is more correct.

So the rule is a **choice**, and the choice is observable:

| | `2^-25` prints as |
|---|---|
| Rust's `{:e}` | `2.9802322387695313e-8` |
| Python's `repr` | `2.9802322387695312e-8` |
| lex-sys | `2.9802322387695313e-8` |

lex-sys follows Steele and White — round the tie up — which is what Rust
does. Python rounds the tie to the even digit. Over the 9000-value corpus
in §5 the two disagree on **three** values, all of them exact ties, and
every one of the six spellings round-trips.

This is worth a section rather than a footnote because it is the kind of
thing a document usually asserts and nobody checks. Both rules were
implemented and both were run against both oracles; the numbers above are
measurements, not recollections.

---

## 4. 1080 bits, measured rather than bounded

The working numbers get large: near the bottom of the subnormals the
numerator picks up a factor of 10³²³ on the way to the first digit, while
the denominator is already 2¹⁰⁷⁶.

The first draft of this comment claimed "about 2200 bits". Instrumenting
the algorithm over the corpus says otherwise:

```
widest 1080 bits, at 4.3374988771064606e-308  →  34 limbs
```

So `limbs()` is 80 — more than double the widest observed, because the
cost of the slack is one bump of an arena pointer and the cost of being
wrong is a silent carry off the top. But the comment now says 1080,
because the comment that said 2200 was made up.

---

## 5. Verified against an oracle

A printer is exactly the kind of code that passes every example someone
thought of and fails on the 4000th random bit pattern. So it is checked
against Rust's `{:e}` over a generated corpus, in
`shortest_printing_agrees_with_an_oracle`.

The test has a pleasant property: **the expected output is the input**.
`{:e}` is the shortest round-tripping form, and the lexer's
`f64::from_str` is correctly rounded, so writing each value into the
generated program as `{:e}` and expecting the program to print that same
string back tests the digits and nothing else. A failure is a
disagreement about digits, never about notation.

The corpus is 9000 values:

| Group | Count | Why |
|---|---|---|
| Every normal power of two | 2046 | §3.1's uneven-neighbour case, exhaustively, not sampled |
| Every subnormal power of two, and the top of each binade | 104 | Where the exponent stops being an exponent |
| Every power of ten in range | 616 | Where the decimal and binary grids line up worst |
| The ones with a reputation | 14 | `0.1`, `1e23`, `9.999999999999999e22`, `f64::MAX`, `5e-324` |
| Deterministic pseudo-random bit patterns | the rest | The cases nobody thought of. splitmix64 with a fixed seed, so a failure reproduces |

Plus `the_values_with_no_literal_are_spelled` for `inf`, `-inf`, `NaN`
and `-0.0`, which have no literal syntax to generate and so are built
from arithmetic inside the program.

During development this corpus found two real defects: §3.3's missing
inclusive boundary, and §3.4's tie rule. Both were one-line changes and
neither would have been found by an example.

---

## 6. What it costs the caller

```
pub fn float_into[&o](out: &!o [byte], x: float) -> [] int
```

A `[byte]` to write into and the value. It answers how many bytes it
wrote, or `-1` if the buffer was too short — threaded rather than
trapped, because the caller chose the buffer.

**24 bytes is always enough**: a sign, seventeen digits, a point, `e`, a
sign and three exponent digits. The longest output there is is
`-1.7976931348623157e308`, at 23.

No `Heap`. The five bignums and the digits live in a `region`, so the
whole thing is one arena that dies at the closing brace, and a program
with no heap capability can still print a float. That is not an
optimisation — `examples/newton.ls` releases `heap` in its first three
lines, and would not compile otherwise.

---

## 7. What is still open

| Question | Why it waits |
|---|---|
| Positional notation | §1.1. `0.1` as `0.1` rather than `1e-1` needs a switch-over rule, which is a formatting policy and not a digit question. It is a layer on top of this one, and cheap once wanted |
| Reading a float from bytes | The inverse. `std.fmt` writes; nothing parses. It needs the same bignum and the opposite loop, and no program has asked yet |
| A width or a precision | `{:.3}` — round to a stated number of digits rather than the shortest. Different algorithm (the stopping rule goes away), same machinery |
| `float_of_bits` | The inverse of `bits_of`, which would let a test feed exact bit patterns without going through a literal. Not needed while `{:e}` round-trips |

---

## 8. The suite

| Test | Rule | § |
|---|---|---|
| `shortest_printing_agrees_with_an_oracle` | 9000 values print exactly what Rust's `{:e}` prints | 1, 3, 5 |
| `the_values_with_no_literal_are_spelled` | `inf`, `-inf`, `NaN`, `-0e0` | 1.1 |
| `a_short_buffer_is_refused_rather_than_overrun` | `-1` rather than a trap or a partial line | 6 |

| Accepting | Shows |
|---|---|
| `examples/newton.ls` | The residuals printed as floats rather than scaled through `truncate` — the thing this slice was for |
