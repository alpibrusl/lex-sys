# Bulk output

> **Status: settled and built.**
>
> `benchmarks-game.md` §2.1 parked this: *"`io.write_all` is one
> `putchar` per byte, so [fasta and reverse-complement] would mostly
> measure a libc call per character. That is a real finding and a
> separate slice."*
>
> It is worth **12.8×** on output, and the interesting part is not the
> number. The fast path existed all along and needed **more authority
> than the slow one** — which is backwards for a language whose whole
> argument is that authority is the thing you count.

---

## 1. The measurement

Eight megabytes of output, sixty-four bytes at a time, to `/dev/null`,
fifteen runs:

| | min | median |
|---|---|---|
| lex-sys, `io.write_all` (one `putchar` per byte) | 40.5 ms | 42.3 ms |
| C, `putchar` per byte | 32.5 ms | 33.7 ms |
| C, `fwrite` per line | **2.8 ms** | **3.0 ms** |

Two things fall out of those three rows, and they point in different
directions:

- **Per-byte output costs 11× in C too.** 33.7 ms against 3.0 ms is
  libc's own per-call overhead, not anything lex-sys does. A language
  that emits one `putchar` per byte is paying what C pays for the same
  shape.
- **The 42 against 34 is the ordinary backend gap** — 1.25×, inside
  `benchmarks-game.md` §1's range and unremarkable.

So the cost is not that lex-sys writes badly. It is that lex-sys could
not write any other way.

And it shows up in a real program. `examples/base64/` encoding 4 MB:

| | min | median |
|---|---|---|
| lex-sys | 54.3 ms | 55.1 ms |
| GNU coreutils `base64` | 5.8 ms | 5.9 ms |

9.3×, on a program whose actual work is a shift and a table lookup.

---

## 2. The authority asymmetry, which is the actual bug

`examples/serve/serve.ls` already does bulk output:

```
extern fn write[&f, &b](ffi: &f Ffi("libc"), fd: int, buf: &b [byte])
    -> [ffi("libc")] int;
```

A `&r [byte]` crosses the foreign boundary as a pointer and a length
(`reach.md` §3), so libc's `write` takes a whole slice and always could.

**Which means the fast path was reachable, and it cost `Ffi("libc")`.**
`reach.md` §5 is explicit about what that grant is worth: *a library is
not an authority domain*, so `Ffi("libc")` is **every authority at
once** — the filesystem, the network, `exec`, all of it.

So a program that wanted to print quickly had to ask for everything, and
a program that asked only for `Io` was held to one byte per call. The
incentive ran exactly the wrong way: **the cheap thing to grant was the
expensive thing to run.** A capability language that charges more
authority for better performance is teaching the wrong lesson every time
someone profiles.

That is the argument for fixing it in the *primitive* rather than in the
library. No amount of `std.io` cleverness helps: a function holding
`&!i Io` can only call what `Io` authorises, and until this slice the
only thing it authorised was one byte.

---

## 3. The rule

```
write_bytes[&i, &b](io: &!i Io, bytes: &b [byte]) -> [io_write] int
```

**`write_bytes` rather than `write`**, because a builtin's name is taken
from every module at once: `std.buffer` already had a `write`, and
`write is a builtin and cannot be redefined` is what the first build
said. That is a real cost of reaching for a primitive rather than a
library function, and it is worth paying here for §2's reason and worth
noticing every other time.

The same capability as `putchar`, the same effect label, the same
erasure — the `Io` is not passed to libc and has no runtime
representation. It answers how many bytes it wrote.

The IR's own comment on `putchar` said this was coming:

> *"M0/M1 scaffolding: `putchar` is how a program produces output before
> there is any FFI."*

It was half right. What replaced it is not a capability-gated foreign
call — that is what `serve.ls` had, and §2 is why it was the wrong
shape. It is a **second primitive behind the same capability**, which
changes what a grant of `Io` is worth without changing what it permits.

### 3.1 `putchar` stays

It is one byte, it is what `getchar` mirrors, and a parser emitting a
single delimiter should not have to build a slice to do it.
`docs/strings.md` §2's rule that a `byte` has no arithmetic is the
reason `putchar` takes an `int`, and none of that changes.

### 3.2 Nothing about the effect row changes

`write_bytes` performs `io_write`, exactly as `putchar` does. A program's
authority report says the same thing before and after — byte for byte,
which is what the test checks (`authority.md` §2): **a faster program
must not be a more powerful one.**

`foreign_symbols` stays empty, which is the half worth stating out loud.
`write_bytes` does reach libc, but the program neither declared that call
nor gets to choose it: it is the builtin's implementation, exactly as
`putchar` has always been libc's `putchar`. Naming `fwrite` in the report
would ask a reader to audit something they cannot influence, and would
make the report's foreign list mean two different things at once. What
`foreign_symbols` lists is `extern` declarations — the calls a program
went out and asked for.

### 3.3 The input side stays open, on purpose

`getchar` is still one byte in, and there is no `read(io, into)` here.
The reason is `standard-input.md` §3.1's unfinished business: `getchar`
answers `-1` at end of input, and a bulk read has to distinguish *short
read*, *end of input* and *error* — which is the same three-way question
`filesystem.md` §3 defers for `fs_read` and `porting.md` §9.1 put a real
program behind.

Output has no such question: it wrote the bytes or the process is gone.
So the two sides are not symmetric and pretending otherwise would ship
the harder one half-answered.

---

## 4. What it is worth

Re-running §1 with `std.io.write_all` on the new primitive, same
machine, same runs:

| | before | after | C / coreutils |
|---|---|---|---|
| 8 MB of output | 42.3 ms | **3.3 ms** | 3.0 ms |
| `examples/base64/` encode, 4 MB | 55.1 ms | **34.7 ms** | 5.9 ms |

Medians. The first row is the primitive on its own: **12.8× faster, and
within 1.1× of C's `fwrite`** on the same loop. Writing is no longer a
thing lex-sys is slow at.

The second row is why the first one is not the headline. base64 got
**1.6× faster**, not 12.8×, and the missing time is the buffer.
`encode` does not write straight out any more: it fills a 4 KiB slice
and flushes it, so every byte it produces is now a bounds-checked store
instead of a call.

That store is not free. Running §1's loop a third way — same 8 MB, same
one `fwrite` per 64 bytes, but *filling* the line byte by byte from
another slice each time rather than writing a prepared one:

| | median |
|---|---|
| lex-sys, prepared line | 3.0 ms |
| lex-sys, filled byte by byte | 9.2 ms |
| C, filled byte by byte | 3.0 ms |

So 8 M bounds-checked byte stores cost **6 ms** where C's cost nothing
measurable — `cc -O2` turns a 63-byte copy into vector moves and lex-sys
emits the loop. base64 buffers 5.4 MB, so about 4 ms of what the bulk
write saved went straight back into filling the buffer. That is a real
cost and it is still a fifth of what the calls cost.

So the honest shape of this slice:

- The thing §2 called a bug **was** a bug, and it is fixed: no program
  has to buy `Ffi("libc")` to print at a reasonable speed any more.
- The 12.8× is real for a loop that does **nothing but write**. What a
  real program gets is §4.1, and it is much less.
- A program that also computes gets the output part only, minus what
  the buffer costs. base64 is still **5.9× off coreutils** afterwards,
  which is now a question about the encode loop rather than about
  printing: `benchmarks-game.md` §3's question, not this document's.

### 4.1 A correction: volume written is not time spent writing

An earlier version of this section said the 12.8× *"is real for
**output**. A program whose time is output — `fasta`,
`reverse-complement`, anything that formats more than it computes — gets
most of it."*

`examples/sort/` falsifies that, and it is the fairest test available:
it already wrote through `io.write_all`, so it moved onto the bulk path
with **no change to the program at all**. Sorting 9 MB and writing all
9 MB back out, medians of nine:

| | median |
|---|---|
| per-byte `write_all` (pre-#54) | 438.9 ms |
| bulk `write_all` | **360.1 ms** |
| GNU `sort` | 62.6 ms |

**1.22×.** A program that writes nine megabytes — as output-heavy by
*volume* as anything here — got a fifth of one doubling.

The mistake was conflating **how much a program writes** with **how much
of its time it spends writing**. `sort` writes a lot and spends its time
merging. Lined up, the three measurements say the same thing:

| | what it does besides write | gain |
|---|---|---|
| §1's loop | nothing | **12.8×** |
| `base64` | a shift and a table lookup per byte | 1.59× |
| `sort` | a merge sort over 300,000 lines | 1.22× |

So the rule is the dull one: **the gain is the share of the runtime that
was libc call overhead**, and nothing about writing a lot of bytes makes
that share large. A program has to be writing bytes it did almost no
work to produce.

Which leaves `fasta` and `reverse-complement` — named above as the
programs that would "get most of it" — an open question rather than a
prediction. `fasta` computes a linear congruential step and a table
lookup per byte, so it is nearer base64 than §1's loop, and this
document should not have guessed. `benchmarks-game.md` §2.1 is where
that gets settled, by measuring.

One thing the numbers do not excuse: `sort` still makes **two libc
calls per line** on the bulk path, one `fwrite` and one `putchar` for
the newline. 600,000 calls for 300,000 lines, where a batched buffer
would make a few thousand. Whether that is worth the buffer-fill cost
§4 measured is exactly the question base64 answered "barely", and it is
not this slice's to spend.

---

## 5. Open

| Question | Why it waits |
|---|---|
| A bulk `read` | §3.3. It needs the three-way answer `standard-input.md` §3.1 and `filesystem.md` §3 both defer, and that is a milestone rather than a primitive |
| `fs_write` of a slice | Already bulk: it takes the whole `[byte]`. Nothing to do, and worth saying so — the asymmetry was between `Io` and `Ffi`, not between console and file |
| Buffering in `std.io` | `print_int` still emits digit by digit. A buffered writer is a library question now rather than a language one, which is the point of this slice |
| fasta, reverse-complement | `benchmarks-game.md` §2.1's two programs. They were waiting on this and are now writable without measuring libc |
| The byte-copy loop C vectorises | §4's third table: 6 ms of scalar stores against C's nothing. It is the same shape as `benchmarks-game.md` §1's finding that the gap tracks how much of the run is code Cranelift generated, and it is now the biggest thing between base64 and coreutils. A slice-to-slice copy primitive would sidestep it; whether that is the right answer or whether the backend should be doing it is not settled |
