# UTF-8

> **Status: settled and built.** `std/utf8.ls` implements §3; the rules
> below were written first and none of them moved on contact.
>
> `strings.md` §8 files UTF-8 decoding as *"library work over `[byte]`,
> in lex-sys, once there is enough language to write it."* There is. Two
> probes compiled on the first try and one matched GNU `wc -m` exactly,
> so this document is not about what the language is missing.
>
> It is about the one decision a decoder cannot avoid, which is the
> decision `strings.md` §1 avoided on purpose — and which §1 was right
> to say would have to be paid somewhere.

---

## 1. The language is ready, measured

Two things had to be true before a string library was worth designing.

**A decoder is writable.** Forty lines over `&r [byte]` — lead-byte
widths from a mask, continuation bytes from another, a running index —
counting code points in a file of ASCII, Latin, Japanese, emoji and a
combining sequence:

| | bytes | code points |
|---|---|---|
| the fixture | 119 | 79 |
| GNU `wc -m` (`LC_ALL=C.UTF-8`) | 119 | **79** |
| the probe | 119 | **79** |

Nothing was missing. Bit operators (`bitwise.md`), `int_of`, subslices
(`slicing.md` §1) and `len` are the whole requirement, and each of them
arrived for a different program.

**A library can return substrings.** The shape that matters for `split`
is a collection of references *into* the text, which means a region
parameter flowing through a generic:

```
fn split_spaces[&h, &t](heap: &!h Heap, text: &t [byte])
    -> [heap] vec.Vec[&t [byte]]
```

That compiles and runs. `Vec[T: val]` accepts a reference because a
shared reference is `val`, and `&t` binds the substrings to the text
they came from, so none of them can outlive it. No new machinery, and
the escape check does the work it already did for slices.

**And a non-ASCII literal needs no escape.** `strings.md` §8 declines
`\x` and `\u` as claims the design will not make, which sounds like a
cost until you write one: a string is bytes and a source file is
already UTF-8, so `"café 日 😀"` is fourteen bytes and eight code points
with nothing to escape. The fixture is written that way on purpose.

So: **the rest of a string library — `split`, `trim`, `join`, ordering,
case folding — is code, not design.** It needs writing, it does not need
deciding, and this document has nothing to say about it beyond that.

---

## 2. The decision, and why it arrives here

`strings.md` §1 declined a validated string type, and the argument was
precise:

> *A validated string type has to answer what an invalid one **is** — an
> error, a replacement character, an unrepresentable state — and each
> answer costs either a fallible constructor everywhere or a lie
> somewhere. Bytes answer it by not asking.*

That was the right call for the **representation**: `&r [byte]` has no
invalid value, so there is nothing to ask about. But a **decoder** is
exactly the thing that asks. Handed `c0 80`, it must do something, and
§1's three options are not hypothetical — they are three shipping
implementations that disagree.

Seven fixtures, four answers:

| bytes | what it is | GNU `wc -m` | Python strict | Python `replace` | naive probe |
|---|---|---|---|---|---|
| `e2 82 ac` | `€`, valid | 1 | 1 | 1 | 1 |
| `c0 80` | overlong NUL | 0 | reject | 2 | 1 |
| `c0 af` | overlong `/` | 0 | reject | 2 | 1 |
| `ed a0 80` | surrogate U+D800 | 0 | reject | 3 | 1 |
| `f5 80 80 80` | above U+10FFFF | **1** | reject | 4 | 1 |
| `e2 82` | truncated | 0 | reject | 1 | 2 |
| `80` | lone continuation | 0 | reject | 1 | 1 |

Every column is a defensible reading of §1's three options — error,
replacement, unrepresentable — and no two columns agree on a single
malformed row.

**GNU is not usable as the oracle here.** It counts `f5 80 80 80` as one
character, and that sequence encodes a value above U+10FFFF, which is
not a character at all. It also answers `0` for every other malformed
input, which is "unrepresentable, so absent" — a third position again.
`benchmarks-game.md` §1 already had to pick rules rather than trust a
reference; same shape, and worth noting because `wordcount.ls` is
checked against `wc` and **must not be checked against `wc -m`.**

---

## 3. The rule

```
decode[&r](text: &r [byte], at: int) -> [] Step
```

```
enum Step {
    Code(int, int),   // code point, and how many bytes it took
    Invalid(int)      // how many bytes to skip; never 0
}
```

**Rejection is not an option and neither is silence.** A decoder that
traps on malformed input makes reading a file a program can be killed
by, which is `filesystem.md`'s own position inverted — a malformed byte
is an ordinary outcome of reading the world, not a bug in the program.
A decoder that silently substitutes loses the one fact the caller might
need. So `Step` reports it and the caller decides, which is the same
shape `file-handles.md` §3 chose for a read: three outcomes, an enum,
no sentinel.

This is `Result`-shaped and is not a `Result`, for the reason the
`Invalid` payload gives: the caller needs the **width** even in the
failing case, or it cannot advance and the loop does not terminate.

### 3.1 What counts as invalid

Strict, and the strictness is the point:

| refused | why |
|---|---|
| overlong forms | `c0 80` is a second spelling of `00`, and a second spelling of anything is a security bug waiting for a comparison to be done before a decode |
| surrogates `d800`–`dfff` | not scalar values; they exist only inside UTF-16 |
| above `10ffff` | not a code point |
| truncated sequences | a lead byte whose continuations are missing or malformed |
| lone continuation bytes | `80`–`bf` cannot start anything |

`f5 80 80 80` is refused, which is where this departs from GNU and
agrees with Python, Rust and the Unicode standard.

### 3.2 How far `Invalid` skips

The **maximal subpart** rule (Unicode 5.2 onwards, and what Python's
`replace` and Rust's `from_utf8_lossy` implement): skip the longest
prefix that could still have been the start of a well-formed sequence,
and at least one byte.

So `e2 82` is one `Invalid(2)` — the pair was a plausible beginning —
while `c0 80` is two `Invalid(1)`s, because `c0` can never begin
anything and `80` cannot either.

The alternative — always skip one byte — is simpler and turns one
truncated character into two errors, which is the row where my own
naive probe disagreed with everyone. Picking the rule the rest of the
world picked is worth more than picking the shorter one, because the
only reason to have a code-point count is to agree with somebody.

Checked rather than asserted: the widths this rule produces reproduce
the **`Python replace`** column of §2's table on all six malformed
fixtures — one error for `e2 82`, two for `c0 80`, three for
`ed a0 80`, four for `f5 80 80 80`. That column is the target, and the
rule hits it.

And over inputs nobody chose. `utf8_decoding_agrees_with_an_oracle`
runs 2,000 generated cases — well-formed text at every width, raw
noise, valid text with bytes corrupted, valid text cut short — against
**Rust's own** `from_utf8_lossy` and `str::from_utf8`, which implement
exactly §3.2's rule and §3.1's validity. Both columns agree on every
case. Widening one range so surrogates are accepted makes it fail and
name the bytes, so the test is a falsifier rather than a formality.

---

## 4. What it does not do

Not normalisation, not grapheme clusters, not case folding beyond
ASCII, not collation. The fixture in §1 contains `é` — an `e` and
a combining acute — which is **two** code points and one thing a reader
would call a character. Counting code points is not counting characters
and this document does not pretend otherwise; it is what `wc -m` counts
and where the agreement in §1 comes from.

Those are all table-driven, the tables are large, and
`compile-time-data.md` gives a `static` the right shape to hold one when
a program asks. None has asked.

---

## 5. Open

| Question | Why it waits |
|---|---|
| Encoding — code point back to bytes | The mirror of §3 and genuinely easier: there is no invalid code point once §3.1 has refused the ones that are not scalar values. Left out so the two are not designed together, for `bulk-io.md` §3.3's reason |
| Grapheme clusters | §4. A real table and a real specification, and nothing in this repo has needed one |
| Whether `wordcount.ls` grows a `-m` | It is the obvious first consumer and the obvious first mistake: §2 says it must not be checked against `wc -m`, so it would need the fixture and the oracle this document used instead |
| The rest of the library | §1: `split`, `trim`, `join`, ordering, case. Code with nothing left to decide, and `std/bytes.ls` is 125 lines today |
| Ordering beyond byte order | `sort.ls` is `LC_ALL=C` on purpose and locale is excluded (`ROADMAP.md`). Code-point order and byte order **agree** for UTF-8 — verified over 4,006 scalar values, the boundary ones and random — so `bytes.compare` sorts text correctly without decoding it at all, and the decoder is not on the path a sort takes. Worth a fixture when there is a comparison to put one on |
