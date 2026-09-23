# Against a wider set

> **Status: measured.**
>
> `against-c-and-rust.md` reported **1.6×** against C, from two kernels.
> Three more, from the Computer Language Benchmarks Game, say the gap is
> **1.17× to 2.58×** — and that it tracks something legible rather than
> being a constant with noise on it (§4).
>
> Two more Game programs, fasta and reverse-complement, widen that range
> in a direction the others do not: fasta measures **0.54×** — *faster*
> than C — because past a certain point the comparison stops being about
> the backend and starts being about the I/O call underneath it (§7).
>
> This document is also the answer to *"are there official benchmarks?"*:
> **no.** §2 is what exists, what it is worth, and which rules were
> borrowed from where.

---

## 1. Why two kernels was not enough

`against-c-and-rust.md` measured Mandelbrot (compute) and a sieve
(memory) and reported 1.69× and 1.56×. Two numbers that close invite a
single headline, and it got one.

Adding three programs that stress different things turns that headline
into a range:

| | what dominates | lex-sys / C |
|---|---|---|
| **binary-trees** | `malloc` and `free` | **1.17×** |
| **fannkuch-redux** | integer arrays, branches | **1.32×** |
| sieve | memory and cache | 1.56× |
| Mandelbrot | float compute | 1.69× |
| **spectral-norm** | float compute with a division in the inner loop | **2.58×** |

Three of those five are new here. The other two are `benches/three/`,
unchanged.

---

## 2. There is no official benchmark, and what that means

No standards body blesses a cross-language benchmark. What exists:

| | what it is | worth here |
|---|---|---|
| **The Computer Language Benchmarks Game** | The de facto citation: ~10 programs, ~30 languages | Its **programs** are a good source of workloads. Its **numbers** are not a target — entries are hand-tuned by motivated contributors, and several leading ones are hand-written SIMD |
| **SPEC CPU** | Genuinely official, licensed, C/C++/Fortran | Benchmarks *CPUs and compilers*, not languages. Not applicable |
| **are-we-fast-yet** | Marr et al., DLS 2016 | The **methodology**, which this project had already arrived at independently |
| TechEmpower, PolyBench, DaCapo, CoreMark | Web, numeric kernels, JVM, embedded | Domain-specific; none reaches this language yet |

So the rules here are are-we-fast-yet's, not the Game's:

> **The same algorithm, line for line. The same output.** Otherwise
> implement idiomatically.

`against-c-and-rust.md` already said *"the same algorithm, line for line,
not the same task"* — which is that rule, reached by the same reasoning
and a decade later. What the Game contributes is the **programs** and,
more usefully, the **expected outputs**: every one of the three below is
checked against a value the Game publishes, on every run.

### 2.1 Which programs were reachable, and which were not

| | |
|---|---|
| **fannkuch-redux** | Ported. Integer arrays and nothing else |
| **spectral-norm** | Ported, after writing `sqrt` (§3) |
| **binary-trees** | Ported. `box`/`unbox` over a `Heap` |
| Mandelbrot | Already in `benches/three/` |
| n-body | Reachable now that §3's `sqrt` exists; not done |
| **fasta, reverse-complement** | **Ported** — §7. `bulk-io.md` §4.1 withdrew the guess that they would gain a lot from bulk output, and the measurement says the guess was wrong in both directions: `fasta` is not nearer `base64`'s 1.59×, it comes out **faster than C**, at 0.54×; `reverse-complement`, which cannot avoid `getchar` on its read side, lands at 1.19×, inside the ordinary range |
| k-nucleotide | Needs a hash table. Writable, not written |
| pidigits | Needs **bignum division**, which `std.bignum` deliberately does not have (`float-printing.md` §3.2: the one quotient a printer needs is a single digit) |
| regex-redux | Needs a regex engine. Out of reach |

Five of ten, which is itself a measurement of the language's reach.

---

## 3. What porting them needed that did not exist

**`sqrt`.** `floating-point.md` §7 leaves `std.math` over floats open,
so `spectral.ls` carries its own: Newton, which `examples/newton.ls`
already showed converges to the limit of binary64. The C program uses
**the same hand-written Newton rather than libm's `sqrt`**, because
otherwise the comparison would be between an intrinsic and a loop.

**Fixed-precision printing.** The benchmark's answer is nine decimal
places. `std.fmt.float_into` prints the *shortest* decimal that
round-trips — `1.2742199912349306e0` — and a stated precision is
`float-printing.md` §7's open row. So `spectral.ls` carries a nine-line
formatter. Small, and a second vote for that row.

**Nothing else.** fannkuch-redux and binary-trees needed no language
feature that was missing, which is the more interesting half: a
permutation benchmark and an allocation benchmark both fell out of what
was already there.

### 3.1 And one compiler bug, found by a program nobody had written

`spectral.ls` opens a `region` for its vectors, closes it, and later
opens another inside a `borrow` to format the answer. **That crashed the
compiler.**

Arena numbers are handed out in the order the lowering meets `region`
blocks, but the backend kept open arenas on a *stack*. Two **sibling**
regions get numbers 0 and 1, and the second opens after the first has
closed — so the stack had length 0 where index 1 was wanted. In debug it
tripped an assertion that had documented the wrong assumption since
arenas landed; in release it indexed out of bounds.

No program in the repository had two `region` blocks side by side. The
fix is three lines — index by arena number, `None` where not open — and
`sibling_regions.ls` is the fixture that would have caught it.

This is `porting.md`'s lesson again: the bugs are found by the programs
nobody thought to write.

---

## 4. What the range means

The gap is not a constant, and the thing it tracks is legible:

> **The gap is how much of the run is spent in code Cranelift generated.**

- **binary-trees, 1.17×** — the program is mostly inside `malloc` and
  `free`, which is the *same libc* in both builds. The backend has less
  of the run to be slower at.
- **fannkuch-redux, 1.32×** — integer array work, bounds-checked, with
  unpredictable branches. The branches are the processor's problem in
  both languages.
- **spectral-norm, 2.58×** — a tight float inner loop with a division,
  which is exactly where a vectoriser earns its keep and where
  `overflow-cost.md` §3.2's finding bites: an observable trap is not
  reassociable, so the reduction cannot be split across lanes.

Which is the same conclusion `against-c-and-rust.md` reached from two
points — *the 1.6× is Cranelift against LLVM, not the price of safety* —
now with the shape of the dependence rather than one number. **The
falsifier stands and gets sharper**: if an LLVM backend lands and the
*range* does not collapse toward its low end, the claim was wrong.

### 4.1 Report the spread, or the number is not checkable

`scripts/three.py` reports best-of-7: the minimum. That suppresses OS
noise, which is why it was chosen, and it throws away the distribution —
so none of the published ratios carried an interval.

`scripts/game.py` reports the **median and the range**, per build:

```
program            N             lex-sys               C -O2   ratio
                        median  (spread)    median  (spread)
fannkuch          11      3978.2ms ( 2.9%)      3007.6ms ( 1.3%)   1.32x
spectral        2000       598.2ms (11.1%)       231.8ms ( 5.2%)   2.58x
binarytrees       18      1439.5ms ( 1.6%)      1228.3ms ( 1.4%)   1.17x
```

spectral-norm's 11.1% is the row worth looking at: it is the noisiest
build here, and a best-of-N report would have shown none of that.

---

## 5. Open

| Question | Why it waits |
|---|---|
| n-body | §2.1. Reachable now that `sqrt` exists, and it would add a second float-heavy point beside spectral-norm's 2.58× |
| ~~fasta and reverse-complement~~ | **Answered, by measuring** — §7. `fasta` is faster than C; `reverse-complement` is not |
| `std.math` over floats | §3. Two programs have now written their own `sqrt`. `floating-point.md` §7's capability question is still the blocker, and the queue behind it is growing |
| A stated precision in `std.fmt` | §3. `float-printing.md` §7's row, with a second caller now |
| Confidence intervals rather than a range | §4.1. The spread is honest and it is not a statistical model. Georges et al. (OOPSLA 2007) is the standard method; nothing here needs that rigour until a change is claimed on a difference smaller than the spread |

---

## 6. The suite

| Test | Rule | § |
|---|---|---|
| `benchmark_game_programs_print_the_published_answer` | fannkuch, spectral, binarytrees, at the N the Game publishes a value for | 2 |
| `fasta_and_reverse_complement_print_the_published_answer` | fasta at N=1000; reverse-complement fed that output, both against the Game's own reference files | 7 |

| Fixture | Rule | § |
|---|---|---|
| `sibling_regions.ls` | Two `region` blocks side by side, which crashed the compiler until this slice | 3.1 |
| `fasta-1000.txt`, `revcomp-1000.txt` | The Benchmarks Game's own N=1000 reference output for `fasta`, and for `reverse-complement` fed that file as input — fetched from benchmarksgame-team.pages.debian.net and reproduced as fixtures rather than downloaded at test time | 7 |

---

## 7. fasta and reverse-complement, measured

§2.1's last row, and §5's answer: the Game's own spec was reachable this
session (`benchmarksgame-team.pages.debian.net` was not blocked), so both
programs are ported and checked byte for byte against the Game's own
N=1000 reference output — not a value this repository derived, one
downloaded from the Game's own site and committed as
`benches/game/fasta-1000.txt` and `benches/game/revcomp-1000.txt`.

`fasta.ls` draws one linear-congruential step and does one linear search
over a cumulative-probability table per byte — the two things the
benchmark's own description forbids optimising away — into a 60-byte
line buffer, flushed with one `io.write_all` per line
(`bulk-io.md`'s primitive). `revcomp.ls` reads with `getchar`, one byte
at a time: `bulk-io.md` §3.3 is why there is no bulk read to reach for,
so the whole read side stays exactly as expensive as `standard-input.md`
already priced it. Both write sides are the same prepared-line buffer.

```
program            N             lex-sys               C -O2   ratio
                        median  (spread)    median  (spread)
fasta         1000000       121.8ms ( 4.6%)       224.9ms ( 7.4%)   0.54x
revcomp       1000000       208.3ms (16.6%)       174.5ms (15.8%)   1.19x
```

**`fasta` is faster than C, and it says nothing new about the backend.**
`fasta.c` writes with `putchar`, one libc call per byte, because that is
what an ordinary C program computing this algorithm writes and
`benchmarks-game.md` §2's rule is the same algorithm, not a hand-tuned
one. `fasta.ls` cannot write that way at all — there is no per-byte
`Io` primitive cheap enough to reach for, only `io.write_all`, so the
ordinary lex-sys program is the bulk one. The 0.54× is not Cranelift
outrunning `cc -O2`; it is one `fwrite`-shaped call every 60 bytes
outrunning one `putchar`-shaped call every byte, which `bulk-io.md` §1
already measured in isolation (11× in C's own numbers) and which shows
up here because the language leaves no slower way to write.

**`reverse-complement` is the control.** Its read side is `getchar`
either way it could be written — lex-sys has no bulk read
(`bulk-io.md` §3.3) — so nothing shields it from the ordinary backend
gap, and 1.19× lands inside the range the other five programs already
described. The 16.6%/15.8% spread is the widest in the suite, which
`benches/three` reserved for float compute (`spectral-norm`'s 11.1%);
here it is a five-record loop dominated by `malloc`-sized buffer growth
rather than a steady inner loop, and neither language is quiet about it.

So the range this document opened with is no longer 1.17×–2.58×; it is
**0.54×–2.58×**, and the new low end is not evidence the backend closed
any gap. It is evidence that "the same algorithm" can leave two
languages with genuinely different *cheapest* ways to do the same I/O,
and when it does, the ratio measures that instead.

| Bench | |
|---|---|
| `benches/game/` | Three programs, each in lex-sys and C to the same algorithm |
| `scripts/game.py` | Runs them, checks the output every time, reports the spread |
