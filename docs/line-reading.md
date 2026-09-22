# A line reader: measured, and the answer is no

> **Status: a documented no, with one function that was earned anyway.**
>
> `ROADMAP.md` carried a row asking for `std.lines`, on the grounds that
> *"`examples/cut/` and `examples/tally.ls` both read `getchar` into a
> fixed buffer, and two programs hand-rolling the same loop is how the
> last four library functions were found."*
>
> **`tally.ls` has no buffer.** It streams — one byte in, counters up,
> nothing kept. The row was written from a memory of what those programs
> do rather than from reading them, and reading them says the bar this
> repository sets was never met.
>
> Writing it down anyway, because the row pointed at something real:
> `examples/cut/` was **silently wrong** on a long line, in a way worse
> than a length limit.

---

## 1. Counting the askers, by reading them

Five programs call `getchar`. What each does with it:

| | reads | keeps |
|---|---|---|
| `tally.ls` | a byte at a time | **nothing** — counters only |
| `wordcount.ls` | a byte at a time | nothing; its one `alloc_slice` is a needle |
| `base64.ls` | a byte at a time | an *output* buffer, 4096, not input |
| `sort.ls` | a byte at a time | **the whole input**, growing on the heap |
| `cut.ls` | a byte at a time | **one line**, and it was the only one |

So the shape `while c >= 0 { … c = getchar(io) }` is in five programs
and means something different in every one. What is *not* in five
programs, or two, is "read until a newline into a buffer": **one**
program does that, and a library function needs a second asker.

`standard-library.md`'s rule is *a function earns its way in when a
program asks for it*, and the last nine arrived that way. The rule is
only worth anything if the count is taken by reading the programs.

---

## 2. What the row was pointing at, and it is worse than a limit

`examples/cut/` read a line into a **60,000-byte arena slice** — what
fits beside a 1,025-byte field bitmap in one 64 KiB chunk — and dropped
everything past it. The source said *"a longer line is truncated"*, as
though that were the whole story.

It is not. A truncated line loses its **delimiters**, and `cut` without
`-s` passes a line with no delimiter through whole. So the program did
not merely lose the tail of a field — it switched to a different rule:

| first field | GNU `cut -d, -f2` | this program | |
|---|---|---|---|
| 59,998 bytes | `second` | `s` | a field, cut short |
| 59,999 bytes | `second` | *(empty)* | a field, gone |
| 60,000 bytes | `second` | **60,001 bytes of `a`** | the wrong *rule* |
| 70,000 bytes | `second` | **60,001 bytes of `a`** | |

Every row exits **0**.

That is a silently wrong answer, which is the one thing
`defined-behaviour.md` §2.1 says this language exists to refuse — in a
program shipped as an example and checked against GNU on eight field
specs, none of which had a long line.

**The suite was not weak; it was short.** Eight specs is a lot of
coverage of *which fields*, and none of *how long a line*. A test that
varies one dimension is silent about the others by construction, and
that is not a fact about this suite in particular.

---

## 3. The fix, and what it costs

`cut` now grows on the heap, the way `sort.ls` already did. There is no
limit left to get wrong, and it matches GNU at every boundary above.

Measured on 14.7 MB, 400,000 lines, median of seven:

| | | |
|---|---|---|
| arena, 60 KiB limit (wrong) | **152.9 ms** | |
| heap, growing (correct) | **181.1 ms** | **1.18×** |
| GNU `cut` | 38.1 ms | |

**Correctness costs 18%**, and it buys an answer that is right on every
input rather than on every input anyone happened to test.

### 3.1 It also costs authority, and that is the right way round

`cut` released its `Heap` before; now it holds one. Its report gains
`heap`, and `lex-sys authority` says so.

`bulk-io.md` §3.2's rule is *a program must not look more powerful for
having been written better*. This is that rule meeting its mirror image
and not contradicting it: the program is not better-written, it is
**correct**, and correctness here genuinely needs an allocator. A row
that stayed at `[]` while the program grew a buffer would be the lie.

---

## 4. The one function that was earned: `buffer.clear`

A program reading a line at a time has to **reuse** its buffer, and
`std.buffer` had no way to move `used` back — `filled` only goes
forward. So the alternative was `drop` plus `empty` on every line:

| | | |
|---|---|---|
| `buffer.clear`, keeping the allocation | **181.1 ms** | |
| `drop` + `empty` per line | **1,984.7 ms** | **13.0×** |

Thirteen times, on the same input. That is not a convenience being
argued for on taste — it is the difference between reusing an
allocation and making 400,000 of them, and it is why `clear` is a **gap**
in the library rather than a nicety.

One asker, which is below the bar §1 just enforced against `read_line`.
The difference is that `read_line` had an alternative that worked, and
`clear` did not: `cut` could not reuse a buffer at all.

---

## 5. What would change the answer on `read_line`

A second program that reads a line at a time. `wordcount.ls` and
`tally.ls` will not become it — they stream on purpose, and a line
reader would make them slower and no clearer.

The candidates are the ports not yet written: anything with records,
`uniq`, `paste`, a CSV reader. When one of them arrives and writes the
same loop `cut` now has, the function is earned and its shape is already
known — `cut`'s loop, with the buffer lent in.

Until then this is a **no**, and the cheapest way to keep it honest is
that `cut`'s loop is eleven lines and visible.

---

## 6. Open

| Question | Why it waits |
|---|---|
| `read_line` in `std` | §5. One asker, and the bar is two |
| A test that varies line length, not only field spec | §2's real lesson. `cut_reports_a_long_line_like_gnu_cut` covers this program; whether the *other* ports have a dimension nobody varied is a question this document raises and does not answer |
| `buffer.truncate(n)` | `clear` is `truncate(0)` and is what a program asked for. The general one has no asker, and §1 is about what that is worth |
