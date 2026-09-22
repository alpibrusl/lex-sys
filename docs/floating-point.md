# Floating point

> **Status: settled and built.**
>
> `defined-behaviour.md` §8 listed floating point as absent rather than
> undefined, with three questions attached: *IEEE-754 semantics, NaN
> ordering, and whether the optimiser may reassociate (it may not).* §8's
> rule is that each gets its answer in the slice that adds it, so this is
> that answer — plus the one §8 did not ask, which turns out to be the
> only hard one.
>
> `against-c-and-rust.md` §4 is why this is worth doing and why the
> reason is not speed: on the kernel measured there, `f64` was **17%
> slower** than the Q16.16 fixed point this language forced. What was
> missing was ten orders of magnitude of precision, and every numerical
> method's error analysis with it.

---

## 1. `float` is binary64, and that is the whole type

```
let half: float = 0.5;
let big = 6.02214076e23;
```

One floating type, named for what it is rather than how wide it is —
`int`, `byte`, `bool`, `float`. There is no `f32`: a second width is a
second set of conversion rules and a second rounding story, and
`defined-behaviour.md` §8 still defers *"unsigned integers and other
widths"* for the same reason.

**A literal needs a decimal point with digits on both sides, or an
exponent.** `1.0`, `1e9`, `2.5e-3`. Not `1.` and not `.5`: both read as
typos more often than as numbers, and neither is needed.

`1.0` does not collide with `t.0`, the tuple component (`tuples.md` §3.1)
— but the reason is not the one the first draft of this document gave.

It said the lexer could decide from the character before the dot, since
an index follows a name and a literal follows a digit. That is wrong, and
the parser's own test said so within the hour: **`t.0.1`** is two
components, and the lexer reading it left to right sees `t`, `.`, and
then `0.1`, which looks exactly like a float.

So the rule needs one bit of context, stated from the other side:

> **A number immediately after a dot is a tuple index, and does not get
> to start a float of its own.**

One lookbehind of one token. Rust settles the same collision the same
way, and the alternative — making the parser split a float token back
into two indices — moves a lexical question into the grammar.

---

## 2. IEEE-754, in full, including NaN

Arithmetic is IEEE-754 binary64 with round-to-nearest-even. `+`, `-`,
`*`, `/` are the IEEE operations exactly. Infinities and NaNs exist, are
produced where IEEE says, and propagate where IEEE says.

```
1.0 / 0.0        //  +inf, and that is the answer
0.0 / 0.0        //  NaN
```

### 2.1 Which looks like a contradiction, and is not

`defined-behaviour.md` §2.1 refuses wrapping arithmetic on the grounds
that *"a silently wrong answer is worse than a stopped process, because
the wrong answer propagates and the stop does not."* Integer division by
zero **traps**. So why does floating division by zero not?

Because the two are different things wearing the same shape:

> **Wrapping lies about a value. NaN announces the absence of one.**

`int::MAX + 1` has a correct answer, the answer does not fit, and
wrapping hands back a *different number* that looks exactly like a
result. Nothing downstream can tell. That is the silently wrong answer.

`0.0 / 0.0` has no correct answer, and NaN is the correctly-typed way to
say so. It is not a number pretending to be the right one; it is a value
whose entire meaning is "there is no number here", and IEEE-754 specifies
its propagation completely. A program that reaches NaN and prints it has
not been lied to.

Two more reasons the trap would be wrong even if the first argument were
not:

- **It would cost what `overflow-cost.md` §3.2 measured, and more.** A
  trap is observable, so an operation that may trap cannot be
  reassociated or vectorised. Checking every floating operation would
  defeat exactly the loops this feature exists for — and IEEE's design
  *assumes* the opposite discipline: compute the whole array, check once
  at the end. That is what NaN propagation is **for**.
- **C can hand us one.** `Ffi` calls returning `double` exist
  (`reach.md` §3.1 permits scalars), so an unrepresentable NaN would need
  a check at every foreign boundary.

**And defined behaviour is not weakened**, which is the point worth being
precise about. IEEE-754 is a *complete* specification: every operation
has a defined result for every input, NaN included. The undefined
behaviour in C's floating point is not in the arithmetic — it is in the
**conversions** (§4) and in the licence to reassociate (§3). This
document defines both.

---

## 3. The optimiser may not reassociate

§8 predicted this and it stands: `(a + b) + c` is not `a + (b + c)`, and
no flag makes it so. There is no `-ffast-math`, no
`@fast` annotation, and no plan for one.

The reason is the project's, not IEEE's. `defined-behaviour.md` §1
promises reproducibility: the same program gives the same answer, and
`canonical-ast.md` hashes a body so two builds of the same source are the
same program. An optimiser permitted to reassociate makes the answer a
function of the optimisation level, which is a silent difference between
a debug and a release build — the exact failure mode the trapping
arithmetic rule exists to prevent, arriving through a different door.

A program that *wants* a different summation order writes it.
Kahan summation is nine lines and says what it is doing.

---

## 4. Conversions are explicit, and one of them traps

No implicit conversion, in either direction, for the reason
`strings.md` §2 gives about `byte`: the conversion is where the decision
lives, so it is written where a reader can see it.

```
float_of(n: int) -> [] float      // exact to 2^53, round-to-nearest beyond
truncate(x: float) -> [] int      // toward zero; traps on NaN, ±inf, out of range
```

**`float_of` rounds rather than trapping**, which is a deliberate break
from `byte_of`'s refusal to truncate. The two are not alike:
`byte_of(300)` loses the magnitude and hands back `44`, a different
number entirely; `float_of(2^53 + 1)` loses one unit in the last place,
under IEEE's defined round-to-nearest-even. One is a lie about which
number this is, the other is the nearest representable number, which is
what a floating type *means*.

**`truncate` traps on exactly the inputs C leaves undefined**: NaN, ±inf,
and any magnitude at or beyond `2^63`. C says the behaviour is undefined;
this says the process stops. That is §2.1's rule applying where it
belongs — the result of `truncate(NaN)` would be a number, and there is
no number it could honestly be.

The name states the rounding because the rounding is the thing a reader
needs: `truncate` goes toward zero. Round-to-nearest, floor and ceiling
belong in `std.math`, where each can say which it is.

### 4.1 And one that converts nothing

```
bits_of(x: float) -> int          // the same 64 bits, read as an integer
```

Not a conversion at all: no value changes, only the type that reads it.
`bits_of(1.0)` is `4607182418800017408`, which is `1.0`'s sign, exponent
and mantissa laid end to end.

It exists so that **taking a float apart is a program's job rather than
the compiler's**. The sign, the exponent and the mantissa are what a
printer needs, and `float-printing.md` is the proof that having them is
enough: `std.fmt.float_into` is written in lex-sys, on this one
instruction, and nothing else about floats had to move into the compiler
to make it possible.

The inverse, `float_of_bits`, is **not** here. Adding it would be a line
of code; nothing has needed it, and a builtin with no caller is a
builtin nobody has checked.

---

## 5. NaN breaks comparison, and this document is not going to hide it

`==`, `!=`, `<`, `<=`, `>`, `>=` are IEEE's. Which means:

```
let nan = 0.0 / 0.0;
nan == nan            // false
nan < 1.0             // false
nan >= 1.0            // false
```

**`==` is not reflexive, and trichotomy fails.** Both are IEEE-754 as
specified, both are what every numerical library expects, and both are a
real hazard: a sort comparing with `<` over data containing NaN produces
a garbage order rather than an error.

So `is_nan(x: float) -> [] bool` exists, because the hazard has to be
checkable and `x != x` is a riddle rather than a test. Whether a *total*
order belongs in `std.math` — IEEE-754 §5.10 defines one — is §7.

This is the one place in the language where a value does not behave like
a value, and it is inherited rather than chosen. The alternative was
§2.1's trap, which costs more than it is worth.

---

## 6. What the numbers mean

| | binary64 | Q16.16, which this replaces |
|---|---|---|
| resolution | 2.22 × 10⁻¹⁶ | 1.53 × 10⁻⁵ |
| range | ±1.8 × 10³⁰⁸ | ±32768 |
| range and precision | traded automatically | traded by hand, checked by nothing |

`against-c-and-rust.md` §4.2 measured what the difference does to one
program: the fixed-point and `f64` Mandelbrots disagreed by 6,213
iterations out of 39.7 million, because pixels near the boundary land on
the wrong side when the coordinate itself is only good to 1.5 × 10⁻⁵.

---

## 7. Open

> **Printing is no longer here.** It was the first row of this table and
> the one called *"what makes `float` awkward rather than incomplete"*.
> `float-printing.md` closes it: `std.fmt.float_into` writes the shortest
> decimal that reads back to the same bits, in lex-sys rather than in the
> compiler, on one new builtin (`bits_of`). What is left below is the
> rest.

| Question | Why it waits |
|---|---|
| ~~`std.math` over floats~~ | **Half answered — `float-math.md`.** The capability question was the wrong question for `sqrt`: it is *one instruction*, so it reaches no library, needs no `Ffi`, and its row is `[]`. It is a builtin rather than library code for the reason §2 there measures — the two programs that hand-rolled a square root got **58.4%** of values wrong in the last place, and one was wrong by **143 orders of magnitude**, because a correctly-rounded root is not expressible in lex-sys. `sin`, `exp` and `log` are the half that really is about error analysis, and nothing has asked |
| A total order | §5. IEEE-754 §5.10 defines `totalOrder`; the question is whether `std.math` should carry it or whether sorting floats should simply be documented as the caller's problem |
| `f32` | §1. A second width drags conversion rules behind it, and nothing has asked |
| Literal parsing exactness | `0.1` is read by Rust's `f64::from_str`, which is correctly rounded. Worth stating as a contract rather than an implementation detail once there is a second front end |

---

## 8. The suite

| Fixture | Rule | § |
|---|---|---|
| `float_without_conversion.ls` | No implicit `int` → `float` | 4 |
| `float_literal_needs_digits.ls` | `1.` is not a literal | 1 |
| `truncating_a_nan.ls` | Refused where it can be seen; the runtime case is a conformance test | 4 |

| Test | Rule | § |
|---|---|---|
| `truncate_traps_where_c_is_undefined` | NaN, ±inf and out-of-range all stop the process | 4 |
| `float_arithmetic_is_ieee754` | Including that `nan != nan` and `1.0 / 0.0` is infinite | 2, 5 |

| Accepting | Shows |
|---|---|
| `floating_point.ls` | Literals, arithmetic, comparison, both conversions, `is_nan`, and §4.1's `bits_of` — including that `-0.0` keeps its sign where `==` cannot see it |
| `examples/newton.ls` | §6: a numerical method that Q16.16 could not carry, now printing its residuals as floats (`float-printing.md`) |
