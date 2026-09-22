# The folder against the backend

> **Status: a test, and it found one thing — though not the thing it
> was built to find.**
>
> lex-sys computes an expression's value in two places. The constant
> folder (`crates/lex-sys-ir/src/fold.rs`, [`compile-time.md`](compile-time.md))
> works it out while compiling, and the backend emits instructions that
> work it out when the program runs. If the two ever disagree, a program
> prints one thing when its arithmetic happens to be literal and another
> when it does not. That is a silently wrong answer, and it depends on
> how the source was spelled.
>
> The folder's own documentation said a test checked this. The test did
> not exist. This document is about the test that exists now: **5,156
> cases, three ways each, no disagreements.** A correct result is only
> evidence if the test could have failed, so it was also run against
> five deliberately broken folders, and it caught all five. Writing it
> surfaced a different fault: `bits_of` of a NaN printed different
> numbers on the two CI targets.

---

## 1. The test that was cited and never written

`fold.rs` documented `bin`, the function that folds a binary operator,
like this:

> Getting one of these wrong would be the silently-wrong answer the
> language exists to refuse, so `bin_traps_exactly_where_the_backend_does`
> checks them against a running program rather than against this comment.

`grep` finds the name once, in that comment, and nowhere else in the
repository. The one disagreement this test would have caught did ship:
`int::MIN % -1`. The folder refused it as a certain trap, and the
backend computed 0. It was found in #71
([`emitted-checks.md`](emitted-checks.md) §4.2), by someone reading the
disassembly for a different reason. The audit that followed listed the
missing test as item C2(b):

> Differential testing of the constant folder against the backend,
> since #71 found them disagreeing on `int::MIN % -1`. For every
> foldable expression, compare the folded value with the compiled
> result.

The comment now names `the_folder_agrees_with_the_backend` and says
what it used to claim.

---

## 2. What is compared

### 2.1 Every operator, on the operands where the rules have edges

| Operand type | Operators | Operands | Cases |
|---|---|---|---|
| `int` | `+ - * / % << >> & \| ^ == != < <= > >=` | 14 | 16 × 196 = 3,136 |
| `float` | `+ - * / == != < <= > >=` | 14 | 10 × 196 = 1,960 |
| `bool` | `== != && \|\|` | 2 | 4 × 4 = 16 |
| unary | `-int`, `~int`, `-float`, `!bool` | | 14 + 14 + 14 + 2 = 44 |
| | **34 operators** | | **5,156** |

The integer operands are both ends of the range and one step in from
each, the shift amount's edges on both sides (`-64 -63 63 64`), and the
small values every rule has a case for (`-2 -1 0 1 2 3`). The float
operands are both zeros, `0.1` (which binary cannot represent exactly),
the largest finite magnitudes, the smallest subnormal, the smallest
normal, both infinities, and NaN with each sign. The operators are
exactly the ones `fold::bin`, `fold::negate`, `fold::not` and
`fold::bit_not` accept.

### 2.2 Three arms per case, because the folder has two entrances

| Arm | How the operands reach the operator | Who computes it |
|---|---|---|
| **literal** | `fn l17() -> [] int { return (a) op (b); }` | the folder, **during lowering** |
| **call** | `fn c17() -> [] int { return int_add(a, b); }` | the folder, in **`evaluate_calls`** after lowering |
| **run time** | `ival(j / 14) op ival(j % 14)`, where `j` comes from a loop counter | **Cranelift**, when the program runs |

Each arm has to be shown to be doing its job, or the comparison
proves nothing:

- **The literal arm's traps are refusals.** A certain trap is a
  `constant-traps` error, and `check` reports every refusal rather than
  only the first ([`agent-errors.md`](agent-errors.md)). Each case is a
  function on its own line, so a single `check --output json` run gives
  the folder's complete trap set, and the reported line number
  identifies each case. The test also asserts that nothing was refused
  under any other rule.
- **The call arm is folded, not just compiled.** `authority --output
  json` reports `folded_calls`, and the test requires it to equal the
  number of cases that produce a value: **4,720 of 4,720**. If a call
  were not folded, it would be run time compared with run time, and
  would pass for the wrong reason.
- **The run-time operands are invisible to both optimisers.** `ival(k)`
  is a pure function, but `k` is a loop variable, so the folder has no
  constant to evaluate. Cranelift 0.121 has no inliner, and lex-sys
  gives it one function at a time. Neither of them can see an operand.
- **A trap in the run-time arm is found without losing results.**
  Standard output is fully buffered when it is not a terminal, so a
  trap discards every line since the last flush. The first prototype
  lost results this way and reported 1,457 false disagreements. So the
  run-time program writes each result to standard error, which is
  unbuffered, and takes its starting case from `argv[1]`. When a case
  kills it, the harness records the trap and restarts at the next case.
  It needs 437 processes: one for each of the 436 traps, plus the
  final clean run. Every trap is a crash the host has to handle (a
  core dump, or whatever `core_pattern` pipes it to), so the loop is
  bounded three ways. A run that neither finishes nor traps within
  30 s is killed. More than 20 traps the folder did not predict stop
  the test early, so a systematically broken run costs 21 crashes,
  not one per remaining case. The whole phase has a 300 s budget. Each
  stop reports the number of runs, the slowest run, the time per
  trapping run and the host's `core_pattern`. This bounding was added
  after the first CI run on linux-x86_64 sat in the test step for over
  25 minutes with no output. §3.2 has what it then reported. The test also requires every trap to end the process
  with a signal: an exit code would be a different failure, not a trap
  ([`defined-behaviour.md`](defined-behaviour.md) §1).

Every result is printed as an `int`: a `bool` as 0 or 1, and a `float`
as its `bits_of`. Printing float bits means `0.0` and `-0.0` cannot
compare equal by accident, and neither can two different NaNs.

---

## 3. The result

**No disagreements.** The literal and call arms agree with each other
on all 4,720 cases that produce a value, and both agree with the run
time. The folder refuses exactly the cases that trap at run time: 436,
by operator:

| Operator | Traps | Why |
|---|---|---|
| `<<`, `>>` | 126 each | 9 of the 14 amounts are outside `0..64`, for every left operand |
| `*` | 74 | overflow |
| `+`, `-` | 40 each | overflow |
| `/` | 15 | 14 zero divisors, and `int::MIN / -1` |
| `%` | 14 | 14 zero divisors, and **not** `int::MIN % -1`, which is 0 (#71) |
| unary `-` | 1 | `-int::MIN` |

No float operation traps, and nothing traps on `bool`
([`floating-point.md`](floating-point.md) §2).

### 3.1 It fails when it should

A zero from a test that could not have failed means nothing. So each of
these one-line changes was made to `fold.rs`, and the test was run
against each one on its own:

| Mutation | Cases that disagree |
|---|---|
| Remove the `Rem if b == -1` line, which is #71's fix undone | **1**: `int::MIN % -1`, with the folder refusing a program the runtime answers |
| Shift range `0..64` → `0..=64` | 28 |
| `a <= b` → `a < b`, on `int` and `float` | 28: the 14 equal integer pairs and the 14 equal float pairs, `0.0 <= -0.0` among them |
| Arithmetic `>>` → logical `>>` | 24 |
| Float negation `-x` → `0.0 - x` | **1**: `-0.0`, which `0.0 - 0.0` is not. (Before §4's repair it was 3, because the two NaNs differed too.) |

The first row is the exact bug #71 found by reading the disassembly.
This test finds it in about a second.

### 3.2 What it costs

The whole comparison takes about 1.6 s with a release compiler. That
covers three compilations of about 10,000 functions each and 437
processes. Under `cargo test`, where the compiler is a debug build, it
takes about 12 s.

That is on a machine whose `core_pattern` is `core` with a zero core
limit, where a trap costs **2.4 ms**. The GitHub Linux runner pipes
every crash to `systemd-coredump`, and with the bounds above in place
it reported:

```
a run from case 1111 neither finished nor trapped in 30 s: 270 runs,
269 traps, 232.6s elapsed; slowest run 2.7s (from case 1056, trapped);
544.3ms per trapping run; core_pattern
`|/usr/lib/systemd/systemd-coredump %P %u %g %s %t 9223372036854775808 %h %d`
```

That is **544 ms per trap**, about 230 times the local cost, and it
slows down as the crashes pile up, until one run exceeded 30 s. So the
first CI run did not hang. It was paying for 436 core dumps, one at a
time.

The repair took three attempts, and the two that failed are worth
keeping:

| Attempt | Why it looked right | What CI said |
|---|---|---|
| Make the binary `0711` | Linux never dumps a process whose executable its user cannot read. A local check as a *different* user agreed | 532 ms per trap. The owner's read bit was still set, and the owner runs the tests |
| Make it `0100` | Owner-only execute. Checked locally as the owning user | 928 ms per trap, one run 26 s. systemd's `50-coredump.conf`, the file that installs that `core_pattern`, also sets `fs.suid_dumpable = 2`, and at that setting Linux dumps non-dumpable processes too, as root, through the same pipe |
| Set the child's `RLIMIT_CORE` to **1** | Measured below, against the runner's configuration reproduced locally | The fix |

A pipe ignores `RLIMIT_CORE`, which is why 0 does nothing. **1** is the
exception: the kernel reserves it to catch a dump helper that itself
crashes, and it answers `RLIMIT_CORE is set to 1, aborting core` without
starting the helper. This was measured here with a `core_pattern` piped
to a helper that sleeps half a second, which stands in for
`systemd-coredump`:

| `RLIMIT_CORE` | One trap |
|---|---|
| unlimited | 509 ms |
| 0 | 509 ms |
| **1** | **5 ms** |
| 2 | 507 ms |

With a piped `core_pattern` and `fs.suid_dumpable = 2`, which is the
runner's configuration as far as it can be reproduced off the runner,
the whole test finishes in 11.3 s. The trap is
unchanged: the process still dies of `SIGILL` or `SIGFPE`. Only the
dump is skipped, and only on Linux, where
`without_a_core_dump` sets the limit between `fork` and `exec`.

---

## 4. What writing it found: NaN was a different number on each target

[`compile-time.md`](compile-time.md) §6 is what makes folding
legitimate in this language:

> an expression has one value, and it is the same value on every host
> and every target.

The float operands include NaN, and the obvious way to compare two
floats exactly is `bits_of`. That raised a question the differential
test cannot answer, because it runs on one machine: *which* bits does
a NaN have?

IEEE-754 leaves a generated NaN's sign and payload to the hardware, and
the two CI targets make different choices:

| Target | `bits_of(0.0 / 0.0)` | Pattern |
|---|---|---|
| x86-64 | `-2251799813685248` | `0xfff8000000000000`: sign set, which x86 calls the "QNaN floating-point indefinite" |
| aarch64 | `9221120237041090560` | `0x7ff8000000000000`: the architecture's default NaN |

The x86-64 value was measured on the machine that wrote this document.
The aarch64 value is the architecture's documented default NaN, and
`every_nan_has_one_bit_pattern` checks both by computing the hardware's
own `0.0 / 0.0` on whichever machine runs it. The linux-x86_64 and
darwin-aarch64 CI jobs each check one of the two rows.

So `print_int(bits_of(0.0 / 0.0))` printed a different number depending
on where the program ran. The folder was not at fault: there is no
cross-compilation, so the folder always runs on the target and agrees
with it. The fault was in §6's claim, which was false for one builtin.

### 4.1 The repair: one NaN

`bits_of` now answers `0x7ff8000000000000` for every NaN. That is the
positive quiet NaN with an empty payload, and it is also aarch64's
default NaN and RISC-V's canonical NaN. On x86-64 it compiles to two
more instructions and no branch:

```
movq    %xmm0, %rax
ucomisd %xmm0, %xmm0        ; unordered only for NaN
cmovp   CANONICAL_NAN, %rax
```

Two reasons this is the right repair and not a documented exception:

- **Nothing a program can do reaches a NaN's payload.** There is no
  `float_of_bits` ([`floating-point.md`](floating-point.md) §4.1), so a
  lex-sys program cannot construct a NaN with a chosen payload. It only
  ever sees NaNs the hardware generated, and their bits are the
  hardware's accident. Canonicalising throws away nothing the program
  put there. The one exception is a NaN returned by foreign code, which
  sits behind `ffi` and is already reported as unbounded
  ([`under-a-grant.md`](under-a-grant.md) §5.1).
- **`bits_of` is the only way to see a NaN's bits.** `==` and `<` are
  false for any NaN, `is_nan` answers `true` for any NaN,
  `std.fmt.float_into` prints `NaN` before it looks at the bits, and
  `truncate` traps. Fixing the one operation that reads the bits makes
  the property true for the whole language, without adding a check to
  every float operation. Cranelift's `enable_nan_canonicalization`
  setting does the latter: it puts a compare and a select after every
  `fadd`, `fsub`, `fmul`, `fdiv` and `sqrt` in the program, to protect
  the one operation that can observe the result.

The corrected claim in `compile-time.md` §6 now says this.

---

## 5. What this does not cover

- **Only one operator per function body.** The call arm's helpers are
  `return a op b;`. What `evaluate_calls` does with loops, `if`,
  recursion, `static` tables, `byte_of` and `int_of` is exercised by
  the existing compile-time tests, not by this comparison.
- **The folder's traps inside calls.** A call whose evaluation traps
  is left unfolded rather than reported, because the IR has no spans
  ([`compile-time.md`](compile-time.md) §9). That falls back to run
  time, so it is safe by construction and has nothing to compare.
- **The edges of the float space, not its interior.** The operands are
  the edges, not a sample of the interior. Rust's `f64` and the
  hardware run the same IEEE operations, so the interior is where
  disagreement is least likely. A seeded random sweep would cover it,
  and nothing has needed one yet.
- **C2's other two items.** (a) was fuzzing the parser and the checker,
  and (c) was reporting a Cranelift verifier failure as an `internal`
  rule with a span. Neither is done here.

---

## 6. The suite

| Test | What it pins |
|---|---|
| `the_folder_agrees_with_the_backend` | §2 and §3: 5,156 cases across three arms, the trap sets equal, the values equal, every call arm folded, every trap a signal, and the counts (5,156 and 436) |
| `every_nan_has_one_bit_pattern` | §4: nine ways to make a NaN, folded and at run time, all reading back as `0x7ff8000000000000`, and the hardware's own NaN measured on the machine running the test |
