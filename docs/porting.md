# Porting a program that already existed

> **Status: done once, and the once is the point.**
>
> Every program in `examples/` until now was written here, in this
> language, by someone who knew what it could do. That is the weakest
> possible evidence about a language, and an outside reader said so:
> *"the strongest next signal would be porting one non-trivial existing C
> program and publishing what the effect rows and authority report looked
> like in anger."*
>
> So: `base64`, from GNU coreutils 9.4. This is what happened.

---

## 1. The result first

`examples/base64/base64.ls` is byte-for-byte GNU `base64` on encode and
decode, including the 76-column wrapping, the padding, the trailing
newline, and the exit status on malformed input. The conformance suite
pipes the same bytes through both binaries and compares
(`base64_agrees_with_coreutils`): twelve input sizes, both directions,
three malformed inputs, and a megabyte.

131 lines of code. It needed **two things that did not exist** and
**nothing else**.

> **"The program" turned out to be two programs.** The suite passed on
> linux and failed on macOS at the first case: empty input, where GNU
> prints nothing and the BSD `base64` macOS ships prints a newline.
> Neither is wrong. A port targets *an* implementation, and this one
> targets GNU 9.4 — so the comparison now checks `--version` for "GNU
> coreutils" and skips the comparison otherwise, while the round trip,
> the exit statuses and the megabyte still run everywhere. Worth one
> paragraph because "port it and compare against the original" quietly
> assumes there is one original.

---

## 2. What it needed: bits

RFC 4648 §4 is a bit-packing specification, and this language had no bit
operators. That is `docs/bitwise.md`, which exists because of this port
and follows `defined-behaviour.md` §8's rule — *each gets a rule here in
the slice that adds it, before the code that needs one*.

With them, the whole of the encoding is four lines:

```
column = emit(io, int_of(table[(bits >> 18) & 0x3f]), column);
column = emit(io, int_of(table[(bits >> 12) & 0x3f]), column);
column = emit(io, int_of(table[(bits >>  6) & 0x3f]), column);
column = emit(io, int_of(table[ bits        & 0x3f]), column);
```

Which is the C, with `int_of(table[...])` where C writes `table[...]`.

**And hexadecimal literals**, which is a smaller thing that turned out to
be the same thing: `& 0x3f` written `& 63` is a mask nobody can read, and
`0xffff_ffff` written in decimal is worse. `bitwise.md` §1.1.

Neither was a design question anybody had been avoiding. Both were absent
because nothing had asked.

---

## 3. What it did not need

No new capability. No library. No change to linearity, regions,
monomorphisation, effect rows, or the standard library. The port used
`std.io` and nothing else, and the six capabilities are the same six.

That is the more interesting half of the result, and it is the half that
could not have been known without doing it.

---

## 4. The rows in anger

```
$ lex-sys authority examples/base64/base64.ls --std
performs
    args
    io_read
    io_write
never touches
    the filesystem
    the heap
    foreign code
```

Three lines, and every one of them is *right* in a way that is checkable
from outside the program: a codec reads its input, writes its output and
parses a flag. It opens no file. It allocates nothing. It calls no C.

Compare `reach.md` §5, where the same tool says `ffi("libc")` about a web
server and cannot say `net`. **The difference is not the tool, it is
whether the authority the program uses has a name** — and `base64`'s
authority is entirely made of things that do: `Io`, `Args`. A program
built from named authority gets an exact report; a program that reaches
through `Ffi` gets a coarse one. This port is the positive control for
that claim, and the web server was the negative one.

### 4.1 And the heap row is load-bearing, not decoration

`never touches the heap` is true because the program **streams**: three
bytes in, four characters out, nothing buffered. That was not a stylistic
choice. An arena is one 64 KiB chunk (`heap.md`), standard input is not
bounded by 64 KiB, and the only way to write this program at all was the
way the C writes it.

A constraint produced the right architecture, which is a nicer outcome
than it sounds and is worth one sentence of scepticism: it produced the
right architecture *here*, for a codec, where streaming is natural. It
would have produced an awkward program for something that needs the whole
input at once, and that program would have had to reach for `std.buffer`
and grown a `heap` row saying so.

The megabyte in the test is the evidence rather than the claim: 1 MiB
through a 64 KiB arena, round-tripped.

---

## 5. What it found in code that was already here

Two things, and neither was in the port.

**A real bug in `examples/serve/`.** The HTTP server read the request
with a single `read` and routed whatever arrived. One `read` returns what
has arrived, not what was sent — so under load it routes half a request
line and answers 404 to a request for `/health`. The conformance suite
failed about one run in six on a busy machine, and passed thirty times in
a row on an idle one.

It is a bug the port did not cause and did not contain. It surfaced
because this slice ran the suite under heavy load for an unrelated
reason, which is the kind of luck that only happens when there is a suite
to run. `serve.ls` now has `read_request`, which loops to the end of the
request line, and ten consecutive full runs are clean.

**Two tests that had quietly stopped testing anything.**
`tests/reject/unknown_character.ls` asserted that `^` is not a token, and
so did a unit test in the lexer. It is now. Both were caught immediately
— the fixture's stated error no longer matched, and the unit test's
assertion failed — which is exactly what a fixture that *states its own
expectation* is for.

Both use `@` now, with a comment saying the thing worth remembering: a
test whose subject is "this is not a token" has a shelf life, and the
expiry date is whenever the language grows.

---

## 6. What this does not establish

`base64` is 131 lines. It is a real program with real opinions and a real
reference implementation to be wrong against, and it is **small**.

What a larger port would test that this one did not:

| Untested | Why it matters |
|---|---|
| Linearity at scale | `base64` owns almost nothing. A program with resources flowing through twenty functions is where "exactly once" either stays readable or does not |
| Effect-row plumbing at depth | The deepest call chain here is three. `reach.md`'s worry — every helper that transitively prints needs the row — needs a program with a real call graph |
| `borrow mut`'s strictness | It appears once, around the whole body. The friction people report is in code that wants to read a field while writing another, and this program has no such code |
| The heap | Untouched. A port that needs one is the port that exercises `heap.md` in anger |

So this is one data point and it is a good one: a real program, ported
faithfully, verified against the original, needing one missing feature
that was missing for no reason. It is not the claim that everything
ports.

---

## 7. Open

| Question | Why it waits |
|---|---|
| A port with resources and depth | §6's table. The honest next one is something with a file, a heap allocation and a call graph — which is a bigger slice than this was, and the reason to do it is exactly §6 |
| `base64 --wrap=N`, `-i` | Deliberately not ported. They are flag parsing, which is `ROADMAP`'s ordinary work, and adding them would have tested the flag parser rather than the language |
| An unsigned width | Not needed here, because base64 never sets the sign bit of an `int`. A port that hashes or checksums would need one within five lines (`defined-behaviour.md` §8) |

---

## 8. The suite

| Test | Shows |
|---|---|
| `base64_agrees_with_coreutils` | §1: the same bytes as `/usr/bin/base64`, both directions, twelve sizes, three malformed inputs, and a megabyte through a 64 KiB arena |
| `an_http_server_written_in_lex_sys_answers_a_real_request` | §5's bug, after the fix — it is the test that found it |

| Program | Shows |
|---|---|
| `examples/base64/base64.ls` | The port |
