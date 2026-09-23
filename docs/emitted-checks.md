# The checks, read out of the binary

> **Status: an audit, and it found four things.**
>
> [`check-cost.md`](check-cost.md) is subtitled *"what **every** check
> this language emits costs"*, and it priced eight of them. It measured
> C. Each kernel in `benches/guards.c` carries a comment saying which
> lex-sys check it stands for, and not one of those comments had been
> checked against the machine code lex-sys actually produces —
> [`backend-limits.md`](backend-limits.md) audited what Cranelift *can
> be asked for*, one level above this.
>
> So: `lex-sys build --emit obj`, `objdump -d`, one small function per
> check. Six of the eight proxies are exact. Two are not, one of them
> the row that carries the document's headline. And on the way, two
> claims about division turn out to be backwards and one compiler bug
> fell out of writing the probe.

---

## 1. The probe found a bug before it found a number

The audit needs one function per check, which means writing
`int_of(byte_of(n))` somewhere. Written as a call:

```
fn g(n: int) -> [] byte { return byte_of(n); }
...
let t = int_of(g(65));
```

```
error: code generation failed: in `main`: Compilation error: Verifier errors
```

No span, no rule tag, no way for a programmer to act on it — the shape
[`agent-errors.md`](agent-errors.md) exists to eliminate. (Since #78 a
failure like this is an `internal` refusal located at the function, and
`check` reports it too: [`internal-errors.md`](internal-errors.md).) Three
spellings reached it:

| Written | |
|---|---|
| `int_of(g(65))` | `uextend` from a value already 64 bits wide |
| `g(65) == byte_of(66)` | a comparison between two different machine types |
| `fn h() -> [] byte { return g(65); }` | a return value of the wrong width |

One cause. `g(65)` is a pure call on constant arguments, so
[`compile-time.md`](compile-time.md) folds it — and a folded call becomes
a **literal**, while `Expr`'s literals are `int`, `bool` and `float`.
There is no `byte` literal, because `byte_of` is where a byte comes from
(`strings.md` §2). So the fold put an `int`-shaped node where the backend
expects one machine byte.

The repair is to make the **return type** part of the fold's condition
rather than only the arguments. Not to invent a byte literal: a new
literal node moves every hash in the repository
([`canonical-ast.md`](canonical-ast.md) §3), and this is the one call
shape in the corpus that it would buy.

Two things are worth saying about how long it survived. It needs a pure
function returning `byte`, called with constant arguments, in a position
that needs the byte — and **no program here had ever written one**, which
is why 493 tests were green over it. And it was a refusal rather than a
wrong answer, which is the failure mode this language is built to prefer.

---

## 2. What each check actually emits

x86-64, `--emit obj`, prologue and epilogue dropped. The operation itself
is marked `·`; everything else is the guard.

| check | emitted | guard |
|---|---|---:|
| `a + b` | `· add` `seto` `test` `jne` | 3 |
| `s[i]` | `cmp` `jae` `· mov (%rdi,%rdx,8)` | 2 |
| `a << b` | `cmp $0x40` `jae` `· shl` | 2 |
| `byte_of(n)` | `cmp $0xff` `ja` `· mov` | 2 |
| `s[lo..hi]` | `cmp` `ja` `cmp` `ja` `· sub` | 4 |
| `-x` | `· xor` `· sub` `seto` `test` `jne` | 3 |
| `truncate(f)` | `· cvttsd2si` `cmp $0x1` `jno` → (cold: `ucomisd` ×3, `movabs`, `movq`, `xorpd`) | 2 hot |
| `a / b` | `test %rsi,%rsi` `je` `· idiv` | 2 |
| `a % b` | `cmp $-1` `jne` `· idiv` | 2 |

Every guard branches to a `ud2`, which is why a trap here is SIGILL and
not a signal with a meaning.

### 2.1 Six proxies are exact

`benches/guards.c` writes the index check as
`(unsigned long)i >= (unsigned long)n`, the shift as
`(unsigned long)w[i] >= 64`, the narrowing as `(unsigned long)v[i] > 255`
and the subslice as two unsigned comparisons. Those are, instruction for
instruction, what comes out of the binary — one `cmp` and one unsigned
branch each, two for the subslice. The overflow kernel's
`__builtin_saddl_overflow` compiles to the same `seto`/`test`/`jne` that
`overflow-cost.md` §3.4 wrote down.

So six of the eight rows in `check-cost.md` §2 stand as measurements of
the real check. The author of that harness knew the unsigned trick and
used it. The rest of this section is the other two.

---

## 3. `truncate(f)`, which is the headline row

Two problems, one small and one not.

**The small one is its name.** Five documents and the roadmap call this
check `int_of(f)`. `int_of` takes a `byte` and widens it, which is
*always defined* and emits **no check at all**; the float conversion is
`truncate`. `int_of(f)` is not an expression this language has. One
document wrote the wrong name and five copied it — the shape
[`hash-stability.md`](hash-stability.md) §2 found in the effect labels,
in prose instead of in code.

**The large one is the guard.** The C kernel tests the range on every
element:

```c
GUARD(!(x >= -9.2233720368547758e18 && x <= 9.2233720368547758e18));
```

Two `ucomisd` comparisons and two branches, unconditionally, per value.
What Cranelift emits instead is:

```
cvttsd2si %xmm0,%rax
cmp    $0x1,%rax
jno    ok
```

`cvttsd2si` answers `0x8000000000000000` for NaN, for either infinity and
for anything out of range — so `rax - 1` overflows **exactly** on the
sentinel, and one `cmp`/`jno` separates every good value from every bad
one. The six-instruction sequence that distinguishes *which* kind of bad
it was sits behind that branch and runs on no value a real program
converts.

So the two are not the same guard. The C one does more work per element,
and `check-cost.md`'s **3.35×** — *"the worst check in the language, and
nothing had looked"* — is the cost of a guard lex-sys does not emit.

What survives, stated carefully: the number is an upper bound on this
check's cost under a vectorising compiler, and the *direction* of §7's
conclusion is unaffected, because `cvttsd2si` with a data-dependent trap
is no more vectorisable than two comparisons are. What does not survive
is the ranking. 3.35× was the largest number in the table and it belongs
to a spelling nobody runs.

---

## 4. Division, where both claims are backwards

`check-cost.md` §1 lists `a / b` and `a % b` as testing **nothing**, and
§5 says *"`BinOp::Div` lowers to a bare `sdiv` and `BinOp::Rem` to a bare
`srem`, with no comparison emitted at all"* — the language getting its
guarantee "from an instruction that was going to fault anyway". The
kernel comment in `benches/guards.c` says the same.

Both operators emit exactly one comparison, and they compare different
things:

| | emits | so a zero divisor | and `int::MIN`, `-1` |
|---|---|---|---|
| `a / b` | `test %rsi,%rsi` `je` | **SIGILL** — its own `ud2` | SIGFPE — the hardware |
| `a % b` | `cmp $-1` `jne` | SIGFPE — the hardware | **answers 0** |

Measured, not read:

```
1 / 0                    trapped (SIGILL)
1 % 0                    trapped (SIGFPE)
INT_MIN / -1             trapped (SIGFPE)
INT_MIN % -1             exit 0
```

One IR instruction is not one machine instruction, and that is the whole
mistake: `backend-limits.md` §2 correctly read `Opcode::Sdiv.can_trap()`
out of the source, and `check-cost.md` §5 turned a fact about the *IR*
into a claim about the *emitted code*.

`a / b` at **1.00×** is still right, and now for a fourth reason rather
than the third one §5 gave: not because nothing is emitted, but because
one predictable compare is nothing beside a 40-cycle `idiv` that has no
packed form to lose.

### 4.1 `int::MIN % -1` does not trap, and should not

[`defined-behaviour.md`](defined-behaviour.md) §2.3 says `/` and `%` trap
on *"`int::MIN / -1` and `int::MIN % -1` — the quotient has no
representation"*. The reason is right for `/` and does not apply to `%`:
a remainder by `-1` is **0**, for every dividend including `int::MIN`,
and 0 is perfectly representable. The emitted code says so — `cmp $-1`,
and the answer is a `mov $0`.

So the document is wrong and the compiler is right, which is the good
direction for this to be found in. §2.3 is corrected in place.

### 4.2 And the two halves of the compiler disagreed about it

Written with literals, the same expression was a **compile error**:

```
error: this arithmetic overflows; the operands are literals,
       so this can only trap
```

The constant folder used Rust's `checked_rem`, which reports `int::MIN %
-1` as overflow. The backend answers 0. So one compiler gave two answers
for one expression depending on whether a value was written down or
computed — the failure [`compile-time.md`](compile-time.md) §4 is built
to avoid, since folding is only sound while it agrees with running.

Fixed by giving the folder the backend's rule: `a % -1` is 0.

---

## 5. What this does not say

* **Not that `check-cost.md` should not have used C.** lex-sys cannot
  vectorise anything (`backend-limits.md` §4), so asking what a check
  costs a vectorising compiler *requires* a second compiler. The method
  is right; two of the sixteen lines were wrong.
* **Not that the ordering collapses.** `s[lo..hi]` at 2.16× against
  `s[i]` at 1.00× is a matched pair with identical spellings, and §7's
  rule — provable is free, loaded is not — rests on those.
* **Not that these are all the checks.** §1's second table lists the
  once-per-allocation ones, and this audit did not read them.
* **Nothing about aarch64.** Every disassembly here is x86-64. The
  division behaviours are now conformance tests, so CI answers for both
  targets on every commit rather than this document guessing.

---

## 6. The suite

| Test | Shows | § |
|---|---|---|
| `folding_a_byte_returning_call` | §1's three shapes — a widen, a comparison and a return — compile | 1 |
| `the_other_division_traps` | `1 % 0` and `int::MIN / -1`, neither of which anything ran | 4 |
| `a_remainder_by_minus_one_is_zero` | `int::MIN % -1` is 0 at run time **and** folded, so the two halves cannot drift apart again | 4.1, 4.2 |
| `division_by_zero_traps_rather_than_being_undefined` | `1 / 0`; it predates this document and is the one row §2.3 had | 4 |

Every one of them runs on linux-x86_64 and darwin-aarch64, which is the
part that matters: §2's table is a disassembly from one machine, and a
rule that only holds on the machine it was read from is not a rule.
