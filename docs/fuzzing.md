# Fuzzing the compiler

> **Status: a test, and it found four bugs, all in the printer.**
>
> Every other suite here checks programs someone chose to write. A
> fuzzer writes programs nobody chose: it takes every `.ls` file in the
> repository, damages each one a little, and feeds the result through
> the whole compiler. Three properties must hold for every mutant.
> Nothing panics. What parses prints back to the same tree. What the
> checker accepts, the backend compiles.
>
> Over **450,000 distinct mutants**, about 41,000 of them accepted by
> the checker, no stage panicked and the backend never refused an
> accepted program. The printer failed four ways. It dropped the comma
> from `(e,)`, left a label's argument unescaped, printed `-(a / 10)` as
> `-a / 10` (in every release since #43), and printed component 2 of the
> integer `1` as the float `1.2`. Each one is fixed and pinned by a unit
> test that fails without the fix. The first corrected a claim in
> [`tuples.md`](tuples.md), and the last could only be seen by comparing
> trees, not text.
>
> **Corrected (#176).** A fifth bug, found later and not in the printer:
> the compile-time evaluator's recursion-depth guard was sized without
> measuring what it exists to prevent, and a mutant deep enough overflows
> the compiler's own stack before the guard has a chance to refuse
> (§4.5). It surfaced only because adding a corpus file shifted, for the
> fixed seed this suite runs with, which mutant a fixed iteration budget
> reaches — the fuzzer's own coverage was never guaranteed complete, and
> this is what that looks like in practice.

---

## 1. Why, and why this shape

The audit listed three items under C2. (b) was comparing the folder
against the backend ([`differential.md`](differential.md)). (c) was
turning a backend failure into a located `internal` refusal instead of a
crash ([`internal-errors.md`](internal-errors.md)). (a) was fuzzing the
parser and the checker, which is this document. §6 of `internal-errors.md`
says why (c) could not replace it: (c) makes what a fuzzer finds
*reportable*, and does nothing to find it.

The usual tool is `cargo fuzz`. It needs a nightly toolchain and
libFuzzer, and this repository pins stable Rust 1.98.1
(`rust-toolchain.toml`) with no dependencies outside Cranelift. A fuzzer
that needs a second toolchain would run on somebody's laptop, and never
in CI. So this one is an ordinary integration test,
`crates/lex-sys/tests/fuzz.rs`, in under 400 lines. It is stable, has no
dependencies and is deterministic. A seed and an iteration count
reproduce a run exactly on either CI target.

It has no coverage guidance. libFuzzer's main strength is steering
towards inputs that reach new code, and this fuzzer does not do that.
Instead it starts from a corpus that already reaches most of the
compiler and makes small, well-formed edits to it (§2).

---

## 2. The mutator

**Seeds.** Every `.ls` file under the repository, found by walking it:
331 files and 563,500 bytes, from `tests/` (273), `examples/` (26),
`benches/` (20) and `std/` (12). The reject fixtures are included on
purpose. Each one is a program one edit away from a refusal, which is
where a checker's edges are. A fixture the lexer refuses contributes no
tokens.

**Edits.** A mutant is a seed's token stream with one edit, or two or
three edits one time in four. Each edit is one of:

| Weight | Edit |
|---|---|
| 5 in 10 | Replace a token with another of **the same kind**, drawn from the whole corpus: a name for a name, an integer for an integer, an operator for an operator |
| 1 in 10 | Delete a token |
| 1 in 10 | Duplicate a token |
| 1 in 10 | Swap two tokens |
| 2 in 10 | Splice in a run of 1–8 tokens from another file |

The same-kind replacement is the edit that matters. A first version
replaced tokens at random and made several edits per mutant. It
reached the parser's accept path on **3.5%** of mutants, and almost
never reached the checker. Keeping the token kind keeps the program's
shape, so a mutant is usually a *different program*, not a broken one.

**The PRNG** is xorshift64\*, eight lines, with the same output on
every platform.

---

## 3. The three properties, and how far mutants get

Each mutant goes through the pipeline the CLI runs:

1. **`parse`** under `catch_unwind`. A panic is a finding. A refusal
   ends the mutant, which is correct behaviour.
2. **`print`, then `parse` again.** The printed text must parse, to the
   **same tree** (`exprs`, `stmts`, `types` and `items` compared), and
   printing that tree must give the same text. Comparing trees is the
   stronger of the two checks. A printer that loses a parenthesis can
   still reach a fixed point one reprint later, on a program that means
   something else, and §4.4's bug reached one at once. The trees are
   compared with every symbol spelled out as its name. Symbols are
   numbered in the order the parser first meets a name, and the printer
   writes a function's generics in canonical order (`[&h, T]` as
   `[T, &h]`), so the same tree can come back with different numbers.
   Comparing raw numbers reported that as a bug in each of its first
   three long runs.
3. **Parse with the standard library, then `lower_all`** (the checker)
   under `catch_unwind`. A panic is a finding. A refusal ends the mutant.
4. **The `main` shape check** the CLI makes before the backend.
5. **`compile_object`** under `catch_unwind`. A panic is a finding, and so
   is an `Err`: the checker accepted the program, so a backend failure
   is the compiler's bug ([`internal-errors.md`](internal-errors.md)).

The fuzzer also counts how far each mutant got:

| Mutants | Parsed | Checked | Compiled | Time (release) |
|---|---|---|---|---|
| 150,000, seed `0x5eed1e55` | 65,560 (44%) | 13,785 (9.2%) | 12,583 (8.4%) | 151 s |
| 150,000, seed `0x0badcafe` | 65,643 (44%) | 13,788 (9.2%) | 12,539 (8.4%) | 156 s |
| 150,000, seed `12345` | 65,477 (44%) | 13,759 (9.2%) | 12,604 (8.4%) | 152 s |

The test asserts floors of a quarter of these: 10% parse, 2% check and
2% compile. Without them, a change that stopped mutants reaching the
backend would still pass, having tested nothing past the parser. The
report deduplicates findings by stage and message, because a thousand
mutants hitting the same panic are one bug.

---

## 4. What it found

The first four were in the printer. None needed the checker, and none
was a panic. The printer is where the canonical form is written, and
[`canonical-ast.md`](canonical-ast.md) needs it to be exact. Hashes are
computed from the tree, not the text, so no hash was ever wrong. But
`lex-sys print` output that reparses to a different program is a
silently wrong answer, which the language exists to refuse. (Only the
`print` command uses the printer. `lex-sys-id` hashes the tree
directly.)

### 4.1 `(e,)` lost its comma

Found at 30,000 mutants. Minimized: `let pair = (1,);`.

[`tuples.md`](tuples.md) §2.1 says a tuple has two components or more.
`(e)` is grouping, so it said a one-tuple cannot be written. That was
false. The parser has always accepted a trailing comma, so `(1,)`
parses as a one-part tuple, and the checker refuses it. The printer
wrote that tree as `(1)`, which reparses as the integer `1`. **A
program the checker refuses became, after one `print`, a program it
accepts.**

The fix keeps the comma (`print.rs`, `tuple`). The design did not move:
`(e,)` is still refused, by the same rule and now with a message that
names it. `tests/reject/one_tuple.ls` pins the refusal, which "there
is no one-tuple to write" had said could not exist. `tuples.md` §2.1 is
corrected in place.

### 4.2 A label's argument was printed raw

Found at 30,000 mutants. Minimized: `-> [ffi("a\n")] int`.

The tree holds a label's argument with its escapes resolved, the same
way it holds a string literal ([`strings.md`](strings.md) §4). The
literal printer escaped it again, but `effect_row` did not. So a
newline in `ffi("…")` came out inside the quotes, and the result does
not parse: *a string literal may not span lines*. The fix is one call
to the `escape` function the literal printer already used.

### 4.3 Prefix operators bound no tighter than `*`

Found at 60,000 mutants. Minimized: `-(a / 10) * 10`, printed as
`(-a / 10) * 10`.

The printer puts back only the parentheses a tree needs, comparing
binding powers. Prefix operators were `UNARY = 9` and postfix
operators were `POSTFIX = 10`, both written as numbers. That was right
until #43 added the bitwise operators and moved `+` to 9 and `*` to 10.
From then on, a negation of a product was printed as a product of a
negation, and `(a * b)[i]` as `a * b[i]`. **The two constants are now
derived from `*`'s power**, so no new operator can overtake them, and a
unit test checks every binary operator against them.

The two halves of property 2 catch this bug differently. The text
check caught it one reprint late, because `-a / 10` reprints as itself.
The tree check catches it at once. The fuzzer was strengthened to
compare trees after this bug, and the tree check found §4.4.

### 4.4 A numeric literal before a dot

Found at 150,000 mutants, and only by the tree check. Minimized:
`1 . 2`, printed as `1.2`.

`1 . 2` is three tokens and parses as component 2 of the integer `1`.
The checker refuses it, since an `int` has no components. The printer
wrote the base and the dot with nothing between them, so `1.2` came
back as **one float token**. The text was already a fixed point, which
is why the text check could not see the bug: `1.2` reprints as `1.2`.
Precedence cannot fix this, because the lexer causes it, not the
grammar. Only a digit after the dot triggers it: `1.x`, `1.e5` and
`1.5.0` already lex as a literal followed by `.`, which was checked
rather than assumed. Even so, every numeric literal before `.` is now
printed in parentheses, `(1).2`. One rule with no exceptions is easier
to keep true than a rule about which characters come next.

### 4.5 The compile-time evaluator's own recursion guard

Found not by a mutation count but by a corpus change: adding
`examples/report/` and `examples/collect/` (#171, #176) shifted, for
this suite's fixed seed, which mutant the fixed iteration budget
reaches. At iteration 2,896 it lands on a fixture recursive `fib`
mutated to `fib(1_000_000)`. Minimized: that call alone, folded at
compile time.

[`compile-time.md`](compile-time.md) §5 already gives the folder a
step budget so a non-terminating computation cannot hang the compiler,
and a separate recursion-depth cap (`fold.rs`'s `DEPTH`, 128) so a deep
recursion spends that budget on steps rather than the compiler's own
native stack. The cap was never measured against what it exists to
prevent. `cargo test` runs each test on its own thread at Rust's
default 2 MiB stack, well under a `main` thread's 8 MiB, and in a
debug build -- uninlined, with every intermediate a stack slot -- 128
levels of the evaluator's own recursion overflows a 2 MiB stack before
the depth check ever gets to refuse. `fib(1_000_000)` never needed the
million: crossing roughly seventy nested calls was already enough,
which the checker reached before the fuel budget did.

The property this breaks is the fuzzer's first one, "nothing panics,"
read too narrowly. A stack overflow is not a panic Rust's `catch_unwind`
can intercept -- it aborts the process outright, which is why this
finding could not be caught, reported and deduplicated the way the
four in the printer were, and had to be bisected by hand instead
(§5's `LEX_SYS_FUZZ_ITERATIONS`, narrowed until one iteration count
crashed and the one below it did not).

The fix lowers `DEPTH` to 32: measured empirically as under half of 68,
the largest value that survives a debug build on a 2 MiB stack (72 is
the smallest that does not), and still comfortably above every
recursion depth a fixture here actually asks the evaluator to fold --
`fib(23)` needs 23 (`running_out_of_fuel_leaves_a_working_program`).
`DEPTH`'s own doc comment now carries this measurement, the same
discipline `FUEL` already had.

---

## 5. Running it

In CI it is part of `cargo test --workspace`: 3,000 mutants, seed
`0x5eed1e55`, under a debug build, in about 15 s. That is a smoke test
that fails the build on any finding. A long run is a local command:

```sh
LEX_SYS_FUZZ_ITERATIONS=150000 LEX_SYS_FUZZ_SEED=0x0badcafe \
    cargo test --release -p lex-sys --test fuzz -- --nocapture
```

Both variables take decimal or `0x` hex. A value that does not parse
**fails the run** instead of falling back to the default. The first long
runs here found that out the hard way: `0x0badcafe` was silently
replaced by the default seed, and the "second seed" repeated the first
exactly, down to the last count.

A finding is written to `$TMPDIR/lex-sys-fuzz/finding-N.ls` as one line
of space-separated tokens. `lex-sys print` makes it readable. A tree
mismatch is headed by `//` lines naming the first node that differs,
before and after. `LEX_SYS_FUZZ_REPLAY=<file>` runs one saved finding
through the same pipeline:

```sh
LEX_SYS_FUZZ_REPLAY=/tmp/lex-sys-fuzz/finding-0.ls \
    cargo test --release -p lex-sys --test fuzz a_saved_finding_replays
```

There is no automatic minimizer. The four bugs above were cut down by
hand, and each became a unit test beside the code it fixed.

---

## 6. What this does not do

- **It does not run what it compiles.** A mutant can loop forever,
  allocate without limit or write to the filesystem through its `World`.
  The backend's *behaviour* is what
  [`differential.md`](differential.md) checks, and it checks it on
  expressions chosen for their edges, not on random programs.
- **It is not coverage-guided** (§1). Its reach is the corpus's reach,
  plus what a few edits can do. A rule that no fixture comes near will
  rarely be exercised by a mutant.
- **It does not minimize.** A tool for that would be worth having once
  findings arrive faster than one person can cut them down by hand.
- **The checker and the backend found nothing.** Over about 41,000
  checked mutants, 37,700 of which compiled, neither panicked and the
  backend never refused an accepted program. That result is only
  evidence up to the mutator's reach, and that reach is §3's 9%, not
  the whole language.

---

## 7. The suite

| Test | What it pins |
|---|---|
| `mutants_never_crash_the_compiler` (`tests/fuzz.rs`) | §3: the three properties on 3,000 mutants, and the reach floors |
| `a_saved_finding_replays` (`tests/fuzz.rs`) | §5: one finding, when `LEX_SYS_FUZZ_REPLAY` names it; nothing otherwise |
| `a_one_part_tuple_keeps_its_comma` (`print.rs`) | §4.1 |
| `a_label_argument_keeps_its_escapes` (`print.rs`) | §4.2 |
| `prefix_and_postfix_bind_tighter_than_every_binary_operator` (`print.rs`) | §4.3, and that no operator binds as tightly as a prefix |
| `a_numeric_literal_before_a_dot_keeps_its_parentheses` (`print.rs`) | §4.4 |
| `tests/reject/one_tuple.ls` | §4.1: `(e,)` is refused, by the rule `()` meets |
