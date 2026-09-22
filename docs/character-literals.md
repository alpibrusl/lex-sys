# A character, written as a character

> **Status: settled and built.**
>
> `bitwise.md` §1.1 added the second spelling of an integer and wrote
> down the rule that made it cheap: *"a hexadecimal literal is a
> **spelling, not a type**."* This document adds the third, for the same
> reason and with the same consequence — and it exists because
> `examples/wordcount.ls` was already writing the translation by hand, in
> a comment, three lines running.

---

## 1. What the corpus spells today

Counted by reading every `.ls` file in `std/`, `examples/` and
`tests/accept/`: **121 places** write an ASCII character as a decimal
number.

| Where | Sites |
|---|---:|
| `std/` and `examples/` | 53 |
| `tests/accept/` | 68 |

The constants are not spread out. One is nearly two thirds of them:

| Sites | Value | Character |
|---:|---:|---|
| 75 | `48` | `0` |
| 12 | `10` | newline |
| 6 | `65` | `A` |
| 5 | `32` | space |
| 3 | `13` | carriage return |
| 2 each | `9`, `45` | tab, `-` |
| 1 each | `44`, `57`, `61`, `69`, `73`, `79`, `85`, `90`, `97`, `100`, `101`, `104`, `116`, `122` | `,` `9` `=` `E` `I` `O` `U` `Z` `a` `d` `e` `h` `t` `z` |

The 75 are one idiom, not 75 decisions: `48 + n % 10`, the digit printer
that every program without `std` has to carry. That it is the same three
lines copied fifty times is a fact about `std` being opt-in, and it is
also why the notation pays here — a reader meeting `48` in a fixture has
no `std.io` nearby to tell them what it is.

And three lines already carry the answer, written by a human, in the
only place the language left for it:

```
let needle = alloc_slice[a](3, byte_of(116));   // 't'
needle[1] = byte_of(104);                       // 'h'
needle[2] = byte_of(101);                       // 'e'
```

That is `examples/wordcount.ls`. A comment that restates the line above
it is the clearest evidence a notation is missing: the information
exists, the programmer typed it, and the compiler cannot see it. If the
number and the comment ever disagree, nothing finds out.

### 1.1 Which is worse than unreadable, twice

Two of the 129 are not merely opaque.

```
if len(flag) == 2 && int_of(flag[0]) == 45 && int_of(flag[1]) == 100 {
```

That is `examples/base64/base64.ls` testing for `-d`. And:

```
return c == 32 || c == 9 || c == 10 || c == 13 || c == 11 || c == 12;
```

That is `std.bytes.is_blank`, and the sixth alternative is a form feed —
which is either correct or a typo for something else, and reading the
line does not tell you which.

---

## 2. The decision: a spelling, not a type

The obvious design is a character literal of type `byte`, because `byte`
is what one unit of text is here (`strings.md` §1). Counting says
otherwise.

| What the site wants | Sites |
|---|---:|
| `int` | **91** |
| `byte` | 30 |

Three quarters want an integer, and it is not close. The reason is in
two places the language already decided:

* `putchar(io, c: int)` and `getchar(io) -> int` mirror libc, so every
  character that crosses the console is an `int` (`standard-input.md`).
* The single most common site, 75 of 121, is `48 + n % 10`, whose `48`
  is arithmetic on an `int` by construction.

So:

> **`'a'` is the integer 97.** It is a third spelling of an integer
> literal, after decimal and hexadecimal. There is no character type, no
> new node, and no new rule about what a character *is*.

`canonical-ast.md` §3 keeps values rather than spellings — `007` and `7`
are one node, `0xff` and `255` are one node — so `'a'` and `97` are one
node too, and §5 below is true for free rather than by effort.

### 2.1 What that costs, stated rather than hidden

The 30 `byte` sites do not lose their conversion:

```
text[i] == byte_of(10)      becomes    text[i] == byte_of('\n')
```

The call stays. What changes is that its argument can be read. That is
the smaller half of the win and it is the honest description of it —
this slice makes 129 constants legible, and removes zero conversions.

A `byte`-typed literal would remove those 30 conversions and add one to
each of the 97 others, which is the same trade run backwards, plus a new
literal node and a moved hash. The counting decides it; the node is why
it is not even close.

---

## 3. The rule

```
'a'    'Z'    '0'    '-'    ' '
'\n'   '\r'   '\t'   '\\'   '\''   '\0'
```

One character between single quotes, or one escape. The value is that
character's byte, as an `int` in `0..=127`.

The escape set is `strings.md` §4's six, with one substitution: `\'`
stands where `\"` does, because the delimiter is what an escape is for.
`'"'` needs no escape and `"'"` needs none either — each literal escapes
its own delimiter and not the other's.

Five things are refused, with the span on the literal:

| Written | Refused because | Rule |
|---|---|---|
| `''` | empty — a character literal holds exactly one | `LiteralForm` |
| `'ab'` | two characters; a string is `"ab"` | `LiteralForm` |
| `'é'` | not ASCII: `é` is two bytes in UTF-8, and this literal is one | `LiteralForm` |
| `'a` | unterminated | `LiteralForm` |
| `'\q'` | not an escape | `UnknownEscape` |

No new rule tags: `agent-errors.md`'s catalogue stays at 52, and both
rules already carry fixtures.

### 3.1 Why non-ASCII is a refusal and not an encoding

`'é'` has an obvious meaning under one reading — U+00E9, 233 — and a
different obvious meaning under another: the first byte of its UTF-8
encoding, 195. `strings.md` §1 declines to make an encoding claim about
`[byte]`, and a literal that silently picked either would make one here.

`utf8.md` is the place that reads multi-byte text, and it reads it from
bytes at run time. A program that wants U+00E9 writes `233`, and a
program that wants its encoding writes the two bytes — both of which say
which one they meant.

---

## 4. What it does not do

* **No character type.** `'a'` is an `int`; `byte` still has no
  arithmetic (`strings.md` §2) and still needs `byte_of`.
* **No `\u` or `\x`.** `strings.md` §4 left both out and the reasons
  have not changed: `\u` is an encoding claim and `\x` is the escape
  hatch `bitwise.md` §1 replaced with a mask.
* **No `\v` or `\f`**, and that leaves two sites behind. `std.bytes`'s
  `is_blank` tests six characters, and this slice names four of them:
  the vertical tab and the form feed have no escape in `strings.md` §4's
  set and stay `11` and `12`. Taking the string's set unchanged is worth
  more than covering that line, and the line now says so where it used
  to say nothing — before, all six were numbers and the sixth was either
  correct or a typo for something else with no way to tell.
* **No multi-character literal.** C's `'ab'` is implementation-defined,
  which is the category this language exists to leave.
* **Nothing about strings.** `"abc"` is unchanged, and a one-character
  string is still a string.

---

## 5. Adding it moved no hash

A character literal is lexed as `TokenKind::Int` and parsed by the same
`int_value` that reads `255` and `0xff`. Nothing downstream of the
parser can tell which spelling was written, so no `SigId` and no
`body_hash` moved.

That claim is not asserted here: `hash-stability.md` found that the 35
golden hash fixtures had never observed an encoder change, because they
landed after the last one. This slice is the second observation of the
right kind and it passes — `a_character_literal_hashes_as_its_integer`
checks the two spellings against *each other* rather than against a
written-down hash, and the golden fixtures check that rewriting **119
sites** across `std/`, `examples/` and `tests/accept/` moved nothing
either.

It is still not the test that would tell us the instrument works, and
saying so is the point of `hash-stability.md` §1: a change designed to
move no tag cannot demonstrate that a moved tag would be caught.

---

## 6. The suite

| Fixture | Rule | § |
|---|---|---|
| `character_literal_empty.ls` | `''` holds no character | 3 |
| `character_literal_two.ls` | `'ab'` is two, and a string is not a character | 3 |
| `character_literal_not_ascii.ls` | `'é'` is two bytes and this literal is one | 3.1 |
| `character_literal_unterminated.ls` | the quote is not closed | 3 |
| `character_literal_bad_escape.ls` | `\q` is not one of the six | 3 |

| Test | Shows | § |
|---|---|---|
| `a_character_literal_hashes_as_its_integer` | `'a'` and `97` are one node | 5 |
| `golden hashes` | rewriting 59 sites moved no identity | 5 |

| Accepting | Shows |
|---|---|
| `character_literals.ls` | the set, the six escapes, `'\''`, `'"'`, and arithmetic on one |
| `std/bytes.ls` | §1.1's six alternatives, named |
| `examples/base64/` | §1.1's `-d` |
