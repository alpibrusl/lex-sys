# Compile-time evaluation

> **Status: settled and built.**
>
> The question behind this document is "is there a way to be *faster*
> than C without giving anything up?", and the first honest answer is
> that we are not yet as fast as C at the easiest thing there is.
>
> `2 + 3 * 4 - 14` is zero. C `-O2` compiles it to `xor %eax,%eax`.
> lex-sys compiles it to a multiply, an add, a subtract, three overflow
> checks and a load from a constant pool. §2 is the disassembly and §2.1
> is why — and the why turns out to be `overflow-cost.md` §3.2's
> mechanism, sighted a second time in a place that document did not look.

---

## 1. The census

`purity.md`'s mistake was worth not repeating: it argued for a
transformation for a page and a half before checking whether any program
could exhibit it. So this is the count first — and then the count again,
after the pass existed, because the first one was wrong.

### 1.1 The estimate, and why it was not the answer

Walking the lowered IR of **60 programs** and marking every operator
whose operands were literals, and every call to a **pure** function whose
arguments were all constant:

| | count |
|---|---|
| Expressions | 9456 |
| Operators that fold to a literal | 77 |
| Calls to a pure function with every argument constant | 65 |

That is 1.5% of the expressions in the repository, none of it in an inner
loop. On the strength of the estimate alone the answer would have been
"not worth it", and §2 is why it was worth it anyway.

### 1.2 What actually folds

`scripts/folded.py` asks the compiler instead of re-deriving the answer —
`lex-sys authority --output json` reports what the pass did — over
**64 programs**:

```
64 programs
  operators evaluated   91
  calls evaluated       15
  provably pure         163 of 529
```

Both numbers moved, in opposite directions, and the second one moved a
long way:

- **Operators: 77 estimated, 91 actual.** Folding is recursive, so
  folding a child exposes a parent the estimate never saw.
- **Calls: 65 estimated, 15 actual.** This is the one worth reading
  twice. "The function is pure and its arguments are constants" is
  **necessary and not sufficient** — the body also has to be something
  the evaluator can run, and §3.1 keeps memory out of it. Fifty of the
  sixty-five call a pure function that builds a struct, matches an enum,
  indexes a slice or opens a `region`, and the evaluator declines every
  one of them.

So the estimate over-counted the interesting half by more than four
times, in exactly the direction that would have made this look like a
better idea than it is. The number to quote is **15**.

---

## 2. What we emit today

```
fn main(..) -> [] int { return 2 + 3 * 4 - 14; }
```

```
mov    $0x3,%eax
imulq  0x48(%rip)          # a constant pool entry, for the literal 4
seto   %sil                # did the multiply overflow?
test   %sil,%sil
jne    <ud2>
mov    $0x2,%eax
add    %r9,%rax
seto   %dil                # did the add overflow?
test   %dil,%dil
jne    <ud2>
sub    $0xe,%rax
seto   %cl                 # did the subtract overflow?
test   %cl,%cl
jne    <ud2>
```

Fifteen instructions and a memory load to compute a constant the parser
already had. C `-O2` emits one:

```
xor    %eax,%eax
ret
```

### 2.1 And the reason is the trap, again

Cranelift runs at `opt_level = "speed"`, and its egraph pass does
constant-fold arithmetic. It does not fold *this* arithmetic, because
these are not `imul` and `iadd` — they are `smul_overflow` and
`sadd_overflow` followed by `trapnz`, and the folding rules are written
for the plain forms.

`overflow-cost.md` §3.2 found that the trap's real cost is not the
never-taken branch but that **a trapping operation is opaque to the
optimiser**; there it blocked the vectoriser. This is the same sentence
in a second place, and it is worth recording because §3.2 presented the
vectoriser as *the* case and it was one case.

**This one is entirely ours, and it is free.** The optimiser cannot fold
`a * b` because it does not know whether it traps. The compiler front end
can, because it has the literals: evaluate it, and either it fits — emit
the answer, with no check, because a constant needs no guard — or it does
not, and §4 says what happens then.

---

## 3. The rule

> A call may be evaluated at compile time when the callee is **pure** and
> every argument is a **constant**. An operator may be folded when its
> operands are.

Purity is not a new judgement. `Func::is_pure` already exists and is
already checked (`purity.md` §2): the `performs` row is empty *and* no
parameter reaches a unique reference. 35% of the functions in this
repository qualify, and nothing needs annotating.

That is the difference from the two languages next door. C has nothing —
C23's `constexpr` applies to objects, not functions, and
`__attribute__((const))` is a promise nothing verifies. Rust has `const
fn`, which is exactly this, but it is **opt-in and restricted**: the
programmer writes `const`, and the body is limited to what const-eval
supports. Here the fact is already computed for every function, so the
set of candidates is not a subset someone remembered to mark.

### 3.1 What is deliberately not in it

- **No `const` keyword, and no request to evaluate.** There is nothing
  to write. A program that becomes foldable stops emitting the work,
  and one that does not, does not. Adding a way to *demand* it would
  make the fuel budget in §5 part of the language.
- **Nothing impure, at any cost.** A function performing `io_write` is
  not evaluated even with constant arguments, and not even if the
  compiler could buffer the output. The row is the gate.

  Worth noticing: in this language that guard can almost never *fire*.
  Every effect needs a capability, a capability is always a parameter,
  and a parameter holding a capability is never a literal — so "impure
  **and** every argument constant" is close to unreachable by
  construction. The check stays because the second half of the purity
  predicate (`purity.md` §2) is the half that does fire: a function may
  declare `[]` and still write through a unique reference.
- **No compile-time allocation**, so no compile-time *data* — §7.

---

## 4. A trap at compile time is a compile error

This is the part that is not about speed.

```
let x = 9223372036854775807 + 1;
```

Today that compiles cleanly and the program dies with `SIGILL` when it
runs. The overflow is certain, the value is in the source, and the
compiler had everything it needed to say so.

So: **when an evaluation traps, the program is refused**, with the
operation and the reason. `defined-behaviour.md` §2.1 says an operation
with no right answer stops rather than inventing one; this moves the stop
from run time to compile time for the cases where it is inevitable, which
is strictly better and costs nothing.

It applies to every trap the language has, not just overflow: division by
zero, an out-of-range shift, `truncate` of a NaN, an index past the end
of a literal slice. If the answer is a certain trap, the answer is a
diagnostic.

The converse is the important half: **a trap that is merely possible is
untouched.** `a / b` with a runtime `b` still emits the check. Nothing
here weakens a guarantee; it only moves guarantees that were already
decided.

### 4.1 Where it is written, not where it runs

One case decides the shape of the rule:

```
if condition { let x = 1 / 0; }     // condition is a runtime value
```

The trap is certain *if the branch is taken*, and the branch may never be
taken. Refusing this refuses a program that would have run.

It is refused anyway. **An expression with no value is malformed where it
is written**, in the same way a type error is — its position in the
control flow is not what makes it wrong, and a compiler that accepted
`1 / 0` in one place and rejected it in another would be asking the
programmer to reason about which branches the optimiser thinks are
reachable.

This is a real narrowing, it rejects programs that ran before, and saying
so is better than discovering it. The alternative — refuse only where the
expression is definitely reached — makes the diagnostic depend on the
precision of a reachability analysis, which is exactly the kind of thing
`defined-behaviour.md` exists to keep out of the language.

---

## 5. Fuel, and what running out means

Evaluation is bounded by a step budget. Two rules:

1. **Running out is never an error.** The compiler stops, discards the
   partial evaluation, and emits the call as ordinary runtime code. The
   program behaves identically either way, which is exactly why the
   budget can be a compiler tuning parameter and not a language rule.
2. **The budget is per call site**, not per program, so a build's
   behaviour does not depend on the order in which functions were
   reached.

This is the difference between a fuel budget and a language feature. If
running out were a compile error, every program would carry an invisible
dependency on the compiler's tuning, and moving a computation into a
helper could break a build. Silent fallback has neither problem — the
only observable effect is how fast the result runs.

### 5.1 Where the dial is set, and what it costs

The budget is **one million steps**, and it was chosen from a
measurement rather than from taste. A naive recursive `fib` is the
convenient yardstick because its cost is `2·F(n+1) − 1` calls:

| | calls evaluated | added build time |
|---|---|---|
| `fib(20)` | 21 891 | 0.02s |
| `fib(23)` | 92 735 | 0.08s — **the last one that folds** |
| `fib(24)` | 150 049 | — budget exhausted, the call is emitted |
| `fib(26)` | 392 835 | 0.24s, if the budget allowed it |
| `fib(30)` | 2 692 537 | 1.69s, if the budget allowed it |

The interpreter runs at about **1.6 million calls a second**, linearly,
so a million steps is roughly 65 ms of build time as a worst case per
call site. `fib(30)` would fold, and it would cost 1.7 seconds to save a
call that takes about five milliseconds to run — which is a good trade
for something built once and run often, and a bad one for a program
built once and run once. A compiler cannot tell the difference, so it
takes the cautious side.

The budget is counted in **steps rather than seconds** on purpose. A
time limit would make the same source produce different binaries on
different machines; a step limit makes the build reproducible, which is
the property `canonical-ast.md` cares about and a stopwatch would
quietly break.

---

## 6. Why this is sound here and shaky in C

A compiler that evaluates at compile time is running the program on the
**host** and shipping the answer to the **target**. That is only
legitimate if the two agree, and C's do not have to:

| | C | lex-sys |
|---|---|---|
| Integer width | implementation-defined | 64-bit, fixed |
| Signed overflow | **undefined** | traps (§4) |
| Float format | not required to be IEEE-754 | IEEE-754 binary64, stated |
| Reassociation | permitted under `-ffast-math`, and contraction by default | **never** (`floating-point.md` §3) |
| `a * b + c` → `fma` | permitted (`FP_CONTRACT`), and changes the result | not emitted — verified in the disassembly |

Every row of the right-hand column was decided for a different reason,
and together they say something this document did not have to argue for:
**an expression has one value, and it is the same value on every host and
every target.** That is what makes running it early a compilation step
rather than a gamble.

The float row is the one worth checking rather than asserting, and it was
checked: `a * b + c` compiles to `mulsd` then `addsd`, not to `vfmadd`.
Contraction is not reassociation and `floating-point.md` §3 does not
mention it, so it was a real hole in the argument until the disassembly
closed it. If a backend ever does contract, compile-time evaluation must
be turned off for floats or the backend told not to — and this row is why
someone will know to look.

There is one more, quieter reason: the AST is canonical and
content-addressed (`canonical-ast.md`), so an evaluation is keyed by the
hash of what was evaluated. Two identical calls are one computation, and
a cache across builds is a later possibility rather than a redesign.

---

## 7. What this is worth against C, honestly

C was measured rather than assumed, because "the compiler will not do it"
is the kind of claim that is thirty years out of date. At `-O2`:

| Shape | C `-O2` | Verdict |
|---|---|---|
| `2 + 3 * 4 - 14` | `xor %eax,%eax` | **C wins today.** This is catch-up, not an edge |
| `poly(5) - 47`, small non-recursive callee | `xor %eax,%eax` — inlined, then folded | **Parity.** We would match, not beat |
| `sum_to(1000000)` | `movabs $0x746a4ae6e0` | **Parity, and not for the reason it looks.** clang did not run the loop; scalar evolution solved it in closed form |
| `fib(23)` | **a runtime call** | **The edge.** clang gives up on recursion whatever the depth, and emits the call. lex-sys folds it (§5.1) |

So the honest claim is narrow and it is not the headline anyone wants:

> Compile-time evaluation brings constant arithmetic **up to** C, matches
> C on the calls C already inlines, and beats C exactly where C's
> analysis gives up — recursion, bodies too large to inline, and
> computations with no closed form for the optimiser to find.

`purity.md` §4.2 said the row's advantage needs a compilation boundary
LLVM cannot see past, and demoted it. That argument was right about CSE
and hoisting and does not apply here: **evaluation needs no boundary, only
purity and constant arguments.** This is the axis §4.2 did not consider,
and it is narrower than the one §4.2 gave up on.

---

## 8. The shape where it would be worth real money

> **Corrected.** This section called `examples/base64/`'s decode table
> *"the one that pays"* and said the feature that reaches it was the next
> document. The feature was built (`compile-time-data.md`) and the first
> thing the measurement found was that **this section overclaimed**: the
> table is worth 5.7×, and the table was already writable without any
> language change. §1 of that document is the correction, and what is
> below is the original observation, which was right about the table.

`examples/base64/base64.ls`, as it was:

```
fn value_of(c: int) -> [] int {
    let table = alphabet();     // 64 bytes
    var i = 0;
    while i < len(table) { if int_of(table[i]) == c { return i; } i = i + 1; }
    return 0 - 1;
}
```

A **linear scan of 64 entries, once per decoded character.** GNU
`base64` uses a 256-entry lookup table, and the file's own comment said
why this one did not: *"64 entries of a 256-entry table is a lot of
source for something a loop settles."*

That is the real-world instance of "the compiler gives up", and it is
everywhere in systems code: CRC tables, decode tables, bit-count tables,
sine tables. C programmers hand-write them or generate them with a
script, *precisely because* the compiler will not.

Compile-time evaluation as designed above does **not** reach it. Every
call to `value_of` has a runtime argument, so nothing folds. What reaches
it is compile-time **data**: a pure function that returns a slice,
evaluated once at compile time and emitted as static bytes.

`compile-time-data.md` is that, and it is now built — a `static` item,
and `examples/base64/` decodes 6.1× faster than it did. What this
section got wrong is *who* the 5.7× belonged to.

---

## 9. Open

| Question | Why it waits |
|---|---|
| Compile-time data | §8. The one that pays. Needs allocation during evaluation and static emission, which is a milestone rather than a pass |
| ~~Reporting what was folded~~ | **Done in this slice.** `lex-sys authority` prints `evaluated at compile time` and `--output json` carries `folded_operators` and `folded_calls`. It is also the only portable way to test the pass: CI builds on two platforms and a disassembler is not among the things they share |
| A trapping **call** is not reported | §4 refuses a trapping *operator* where it is written, because lowering still has the span. The call pass runs afterwards, over an IR that carries no spans, so a pure call that traps on its constant arguments is left alone and traps at run time as it did before. The asymmetry is real and this is the note admitting it; closing it means spans in the IR |
| Caching across builds | §6's hash makes it possible. Nothing has asked, and a cache that is wrong is worse than no cache |
| Folding through `if` on a constant | Dead-branch elimination is the same fact applied to control flow. Cranelift does this one already, so it is redundant until it is not |

---

## 10. The suite

| Fixture | Rule | § |
|---|---|---|
| `constant_overflow.ls` | A certain overflow is refused where it is written | 4 |
| `constant_division_by_zero.ls` | Every trap kind, not just overflow | 4 |
| `constant_shift_past_the_width.ls` | …including `bitwise.md` §3's range | 4 |
| `constant_trap_on_a_dead_branch.ls` | The narrowing §4.1 chose, recorded as deliberate | 4.1 |

| Test | Rule | § |
|---|---|---|
| `constants_are_evaluated_at_compile_time` | Operators and a recursive call both fold, read back from the report | 2, 3 |
| `running_out_of_fuel_leaves_a_working_program` | `fib(23)` folds, `fib(24)` does not, and both print the same answers | 5, 5.1 |
| `a_possible_trap_still_traps` | The same three operators on a runtime value still stop the process | 4 |
| `a_folded_float_is_the_same_float` | The folded and unfolded spelling of one expression print identically | 6 |
| `a_shift_past_the_width_traps` | Rewritten: the amount is a `var`, because a literal is now a diagnostic | 4 |

| Accepting | Shows |
|---|---|
| `compile_time.ls` | Arithmetic, a nested call, a recursive call, bit operators — and the same `fib` on a runtime value, so the fixture says the cost changed and the answer did not |

| Script | |
|---|---|
| `scripts/folded.py` | §1.2's census, asked of the compiler rather than estimated |
