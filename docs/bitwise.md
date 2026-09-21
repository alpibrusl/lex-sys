# Bits

> **Status: settled and built.**
>
> `defined-behaviour.md` §8 listed *"shifts and bitwise operators"* as
> absent rather than undefined, with two questions attached: what happens
> at a shift amount ≥ 64, and whether `>>` is arithmetic. §8's rule is
> that each gets an answer *in the slice that adds it, before the code
> that needs one* — so this document exists because a port needed them
> (`docs/porting.md`), and it answers both plus the two §8 did not think
> to ask.

---

## 1. The set

```
a & b      a | b      a ^ b      ~a       a << n      a >> n
```

Six operators on `int`, and nothing on `byte`. `strings.md` §2 refuses
arithmetic on `byte` so that unsigned widths can stay deferred, and a
bitwise operator is arithmetic for that purpose: `byte_of(int_of(b) & 15)`
is what you write, and it says where the range check happens.

That is the same shape `strings.md` §4 predicted — *"a caller that wants
the low eight bits writes the mask explicitly, once there are bitwise
operators to write it with"* — so the sentence that was a promise is now
a program.

### 1.1 And hexadecimal literals, which the same code demanded

Writing `0xff` was not possible until this slice either, and a mask in
decimal is a mask nobody can read:

```
byte_of(int_of(b) & 15)        // fine, because 15 is small
value & 4294967295             // and this is not
value & 0xffff_ffff            // this is
```

`0x` is the one prefix. Base two and base eight were left out on purpose:
a mask is written in hex by everyone who writes masks, `0b` earns its
place only where a bit pattern is the *documentation* (a register map,
which this language cannot yet reach), and `0o`'s only customer is a Unix
file mode, which it cannot set.

A hexadecimal literal is a **spelling, not a type**. `canonical-ast.md`
§3 keeps values rather than spellings, so `0xff` and `255` are the same
node and hash identically — which is why §6 below is true of this too.

---

## 2. `>>` is arithmetic, and that is forced

`int` is signed, and a signed right shift that filled with zeros would
turn a negative number positive. §8 had already called this: *"it will be
arithmetic; `int` is signed."*

```
(0 - 8) >> 1   ==   0 - 4
```

A logical shift is a different operation on a different type, and the
type it belongs to — an unsigned width — is still deferred
(`defined-behaviour.md` §8). When unsigned integers arrive, `>>` on them
will be logical, and it will be the *type* that decides rather than a
second operator.

---

## 3. A shift amount out of range traps

```
x << 64        // traps
x << -1        // traps
x >> 64        // traps
```

C leaves this undefined. Rust panics in debug and masks in release, so
the same program means two things. This language has one answer for the
whole family, and §2.1 of `defined-behaviour.md` already gave it: a
silently wrong answer propagates and a stopped process does not.

The range is `0 <= n < 64`, checked against the shift *amount* and not
against the value. 64 is the width of `int` and the only width there is.

---

## 4. But a shift does not trap on the value it produces

This is the exception, and it is deliberate:

```
1 << 62        //  4611686018427387904
1 << 63        // -9223372036854775808, and this is fine
```

`1 << 63` sets the sign bit. Read as arithmetic that is an overflow, and
§2.1 traps on overflow. Read as bits it is exactly what was asked for,
and refusing it would make `<<` useless for the only thing it is for.

> **The rule: `+` is arithmetic and traps; `<<` is bits and does not.**

Which is the same line `defined-behaviour.md` §2.2 already draws between
`+` and `wrapping_add` — *"`+` is what you write when you mean
arithmetic, `wrapping_add` is what you write when you mean the bits"* —
applied to an operator that has no arithmetic reading at all. A bitwise
operator is a `wrapping_` operator that does not need the prefix, because
nobody reaches for `&` by accident.

`&`, `|`, `^` and `~` cannot overflow, so the question does not arise for
them.

---

## 5. Precedence is Rust's, not C's

Loosest to tightest:

```
||
&&
==  !=
<  <=  >  >=
|
^
&
<<  >>
+  -
*  /  %
```

**Bitwise binds tighter than comparison.** In C it does not, so

```c
if (flags & MASK == 0)      /* C: flags & (MASK == 0) */
```

is a bug that every C style guide warns about and every C programmer has
written. It is a famous defect, C keeps it for compatibility with code
written before `&&` existed, and there is no compatibility to keep here.

So `flags & mask == 0` means `(flags & mask) == 0`, which is what it
looks like.

Shifts bind tighter than `&` and looser than `+`, which is Rust's
ordering and reads correctly for the thing shifts are mostly used for:
`base + n << 2` would be surprising either way, so §7's fixture pins it
rather than leaving it to be discovered.

### 5.1 `&` in expression position is unambiguous

`&` is the reference constructor — `&r T`, `&!r T`, and a region binder
`fn f[&r]`. All three are *type* positions, and lex-sys has never let a
type and an expression meet: a type appears after `:`, after `->`, or
inside `[...]` on a declaration, and nowhere else.

There is also no address-of operator to collide with, because a reference
here comes from `borrow` and from nothing else
(`linearity-and-effects.md` §5). So a `&` between two expressions can
only be the binary operator, and the parser needs no lookahead to know
it.

`>>` is free for the same structural reason: generic arguments are
written in brackets, `Pair[int, Vec[int]]`, so the closing-angle-bracket
problem that C++ spent a decade on does not exist here.

---

## 6. Adding these moved no hash

An operator is hashed by a code inside `Binary`/`Unary`
(`crates/lex-sys-id`), not by a node tag of its own, and the six new
codes are appended after the thirteen that existed. So every `SigId` and
`body_hash` in the repository is byte-identical to what it was before
this slice, and `docs/INVARIANTS.md` needed no entry.

That is worth one line because it is the property the canonical AST was
designed for and this is the first slice that got to *check* it rather
than assert it: `ids_are_stable_across_the_operator_set` compares the
hashes of a program written before the operators existed.

---

## 7. The suite

| Fixture | Rule | § |
|---|---|---|
| `bitwise_on_a_byte.ls` | `byte` still has no arithmetic, so a mask says where the range check is | 1 |
| `bitwise_on_a_bool.ls` | `&` is not `&&`, and a `bool` is not an `int` | 1 |

An out-of-range shift is **not** a reject fixture, for the reason
`slicing.md` §8 gives: the reject harness runs `check`, and a program that
traps is one that compiled. Those are conformance tests.

| Test | Rule | § |
|---|---|---|
| `a_shift_past_the_width_traps` | `n >= 64` and `n < 0` both trap, and `1 << 63` does not | 3, 4 |
| `ids_are_stable_across_the_operator_set` | Adding five binary codes and one unary code moved no hash | 6 |

| Accepting | Shows |
|---|---|
| `bitwise.ls` | All six, the precedence table, arithmetic `>>`, `1 << 63`, and a hex mask |
| `examples/base64/` | §1's promise as a program: the port that needed them |
