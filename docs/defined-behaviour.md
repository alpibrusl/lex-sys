# Defined behaviour

> **Status: settled for what exists; the open list at the end is what does
> not exist yet.** Written for M3 ([#1](https://github.com/alpibrusl/lex-sys/issues/1)),
> and implemented in the same slice rather than after it — every rule below
> has a fixture, and the ones that end a process have a conformance test
> that runs the binary and checks it died.

C and Rust both leave behaviour open in places, and C leaves it open in the
places that matter most. This document is the list of those places and what
lex-sys defines them to instead.

This is not pedantry, and it is not a safety feature bolted on the side. It
is what makes the rest of the project mean anything: **content-addressing,
replay and attestation are claims about a program's behaviour, and a program
with undefined behaviour has no behaviour to make claims about.** A hash over
source that can compile to two different answers is a hash over nothing.

---

## 1. The rule

> **Every operation either produces a defined result or ends the process.
> Nothing is left to the implementation, the optimiser, or the target.**

There is no third outcome. In particular there is no "this is undefined, so
the compiler may assume it never happens" — the assumption C makes, which is
where its worst miscompilations come from.

Ending the process is a **trap**: the instruction stream contains an
instruction that faults, the operating system kills the process, and the exit
status says so. On both supported targets that is `SIGILL`. A trap is not a
panic: there is no unwinding, no handler, no destructor, nothing to catch.
The program stops.

Trapping is not a cop-out. A trap is *deterministic* — the same input traps
on every target, every run, every build — which is exactly what undefined
behaviour is not.

---

## 2. Integers

`int` is **64-bit, two's complement, signed**. There is one integer type;
unsigned types and other widths are not in the language yet (§8).

### 2.1 Overflow traps

`+`, `-`, `*` and unary `-` produce the mathematically correct result, or
they trap. They never wrap.

```
let n = 9223372036854775807;
n + 1                      // traps
0 - (0 - 9223372036854775807 - 1)   // traps: -int::MIN has no counterpart
```

Wrapping silently is *defined* behaviour — C has it for unsigned types, Rust
has it in release builds — so it would satisfy §1. It is refused anyway, on
the same grounds division by zero is: a silently wrong answer is worse than a
stopped process, because the wrong answer propagates and the stop does not.
A checksum that wraps is doing arithmetic; a balance that wraps is a bug that
will be found somewhere else, later, by someone else.

The cost is real and is paid on purpose: every addition is a checked
addition. A branch that is never taken is close to free on both targets, and
"close to free" is the right price for an answer that is always right.

### 2.2 Wrapping is spelled out

Wraparound is the *intent* in a hash, a checksum, a PRNG or a cycle counter,
and a language that cannot express it forces a workaround worse than the
thing it forbids. So the three wrapping operations exist as builtins:

```
wrapping_add(a, b)
wrapping_sub(a, b)
wrapping_mul(a, b)
```

Each is two's-complement wraparound, defined for every input, and never
traps. The asymmetry is the point: `+` is what you write when you mean
arithmetic, `wrapping_add` is what you write when you mean the bits. You
cannot get the second by accident.

There is deliberately no wrapping division: `/` has no overflow case that
wrapping would help with (see below).

### 2.3 Division and remainder

`/` and `%` trap on two inputs:

* a **zero divisor** — there is no answer, and C's answer is undefined;
* `int::MIN / -1` and `int::MIN % -1` — the quotient has no representation,
  which is the same overflow as §2.1 and traps for the same reason.

Otherwise division **truncates toward zero** and the remainder takes the sign
of the dividend, which is C99's rule and Rust's:

```
 7 /  3 ==  2       7 %  3 ==  1
-7 /  3 == -2      -7 %  3 == -1
 7 / -3 == -2       7 % -3 ==  1
```

### 2.4 Literals

An integer literal that does not fit in `int` is refused by the parser, not
truncated. `007`, `7` and `0_0_7` are the same value and the same hash: the
AST stores the value, never the spelling.

---

## 3. Evaluation order

**Left to right, everywhere, always.** Every rule below is a consequence of
that one, and each is enforced rather than merely intended.

* **Binary operators** evaluate the left operand, then the right. `f() + g()`
  runs `f` first.
* **`&&` and `||` short-circuit.** The right operand is not evaluated when
  the left already decides the answer. These are the only operators that skip
  an operand, and they are control flow rather than instructions.
* **Call arguments** evaluate left to right, all of them, before the call.
  This includes arguments a builtin discards: a capability argument is
  checked and erased, but an expression that computes one still runs.
* **Enum payloads** evaluate left to right, in the order written, which is
  also the order declared — a variant's payload is positional.
* **Struct literal fields** run in **declaration** order, and writing them in
  any other order is *refused* rather than silently reordered:

  ```
  struct P { x: int, y: int }
  P { y: f(), x: g() }     // ✗ refused: `x` is declared before `y`
  ```

  A struct's fields are laid out and evaluated in declaration order, so a
  literal written the other way round would run `g()` before `f()` while
  reading as though it did the opposite. In a language whose whole argument
  is that effects are visible in the text, having side effects reorder
  underneath the text is the wrong trade. **The order you read is the order
  it runs**, and the compiler keeps it that way.

* **Assignment** evaluates the right-hand side first, then the place. For
  `r.f = g()` that means `g()` runs before `r` is evaluated. Nothing in the
  language can currently observe the difference — a place is a binding or a
  field through a reference, neither of which can have a side effect — and it
  is written down here so that it stays true when places grow.

---

## 4. Memory

* **There is no uninitialised memory.** `let` and `var` both require an
  initialiser; the grammar has no form that declares a binding without a
  value. So there is nothing to read before it is written.
* **There are no null references.** A reference comes from `borrow` or from
  `alloc`, and both produce one that points at something.
* **There are no dangling references.** A reference's region is a block, and
  nothing whose type mentions that region leaves it
  (`linearity-and-effects.md` §5, §6). The check is an occurs-check over one
  type, not an analysis.
* **Every index is bounds-checked, and an out-of-range one traps.** `s[i]`
  compares `i` against the length the slice carries, and the comparison is
  unsigned — so a negative index, read as an enormous unsigned one, is
  caught by the same instruction as an index past the end. There is no
  unchecked form and no release mode that removes the check: reading past
  the end of an allocation is the undefined behaviour §1 says this language
  does not have.
* **A slice's length is checked where it is made.** `alloc_slice[a](n, v)`
  traps on a negative `n`, and the byte count `n * stride` is a checked
  multiplication like any other (§2.1) — an overflowing length would
  otherwise ask the arena for less memory than it is about to write.
* **Arena exhaustion traps.** An arena is one 64 KiB chunk; asking for more
  than it holds kills the process rather than writing past the end of the
  allocation (§6 of `linearity-and-effects.md`). Growing the chunk would make
  release a walk instead of one `free`, and that trade has not been made.
* **Allocation failure traps.** If `malloc` returns null when a `region`
  opens, the process dies there rather than storing through it.
* **No aliasing rules to violate.** There is no `restrict`, no
  `noalias` and no pointer provenance to lose, because the language has no
  raw pointers. Two copies of one `&!r` alias, and that is defined:
  they are two copies of one address, and writes through them land in the
  order they run.

---

## 5. Layout

Struct fields are laid out in declaration order. A value is *scalarised*
into leaves — an `int` or a `bool` is one leaf, a struct is its fields'
leaves in order, an enum is a tag leaf followed by the widest variant's —
and where a value has to live in memory, each leaf takes 8 bytes. A
reference is one leaf, a pointer; a *slice* is two, a pointer and a length,
because `[T]` is the one referent whose size is not in its type. A slice's
elements are contiguous, each one leaf-stride apart.

This is deterministic, and the same compiler on the same source produces the
same layout every time. **It is not yet a stability contract.** Nothing
depends on it across processes: no aggregate crosses the FFI boundary (§8.4
refuses one), nothing is serialised, and layout does not reach any hash.
When something does depend on it, the rule and its guarantees go in
`canonical-ast.md` beside the other invariants, and this paragraph gets
shorter.

---

## 6. The foreign boundary

C's behaviour is C's. What this language defines is what reaches it:

* only `int`, `bool` and `()` cross, plus borrowed capabilities, which do not
  cross at all (`linearity-and-effects.md` §8.4);
* `int` at the boundary is the platform's 64-bit integer, not C's `int`;
* a value narrowed on the way out — `putchar`'s character reaches libc as a
  32-bit `int` — is truncated by two's-complement truncation, which is
  defined, and what libc does with it afterwards is C's rule, not ours.

Past that point the guarantee stops, and it stops *visibly*: reaching C at
all requires an `Ffi` capability, and the effect row of every caller above
says so. That is the trade §8.4 makes — C's effects are not eliminated, they
are made impossible to reach silently.

---

## 7. What is *not* defined, and never will be

Nothing. There is no list of "implementation-defined" behaviour and no
"unspecified" category. If an operation exists, this document says what it
does or says that it traps.

---

## 8. What does not exist yet

These are not undefined — they are absent. Each gets a rule here in the slice
that adds it, before the code that needs one:

| Missing | What will need deciding |
|---|---|
| Shifts and bitwise operators | Shift amounts ≥ 64, and whether `>>` is arithmetic or logical (it will be arithmetic; `int` is signed) |
| Unsigned integers and other widths | Conversion rules, and whether unsigned arithmetic also traps (it should) |
| Casts and conversions | Narrowing, and whether a lossy one traps or is refused |
| Floating point | IEEE-754 semantics, NaN ordering, and whether the optimiser may reassociate (it may not) |
| Strings | Encoding, and what an invalid one is |
| Concurrency | Everything. There is none, and a memory model is the price of adding any |

---

## 9. How this is enforced

Not by assertion. Every rule above is a fixture:

| Rule | Where |
|---|---|
| Overflow traps | `crates/lex-sys/tests/conformance.rs` — builds a program and checks the process died by signal |
| Indexing past a slice traps | same, for `xs[5]` and `xs[-1]` on a slice of 3 |
| Division by zero traps | same, and it predates this document |
| Arena exhaustion traps | same |
| Wrapping does not trap | `tests/accept/wrapping_arithmetic.ls` |
| Struct fields run in declaration order | `tests/reject/struct_fields_out_of_order.ls` |
| A slice is a reference, and `[T]` is not a value | `tests/accept/slices.ls`, `tests/reject/unsized_slice_value.ls` |
| An oversized literal is refused | `crates/lex-sys-syntax` unit tests |

A rule with no fixture is a rule this project does not have.
