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
    err_write
    io_read
    io_write
never touches
    the filesystem
    the heap
    foreign code
```

Four lines, and every one of them is *right* in a way that is checkable
from outside the program: a codec reads its input, writes its output,
parses a flag, and complains when the input is not base64.
(`err_write` is the newest and is the whole of `standard-error.md`; this
program exited 1 in silence until then.) It opens no file. It allocates nothing. It calls no C.

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

| Untested by `base64` | Why it matters | Answered in |
|---|---|---|
| Linearity at scale | `base64` owns almost nothing. A program with resources flowing through a call graph is where "exactly once" either stays readable or does not | §9.2 |
| Effect-row plumbing at depth | The deepest call chain here is three. `reach.md`'s worry — every helper that transitively prints needs the row — needs a program with a real call graph | §9.3 |
| `borrow mut`'s strictness | It appears once, around the whole body. The friction people report is in code that wants to read one thing while writing another | §9.4 |
| The heap | Untouched. A port that needs one is the port that exercises `heap.md` in anger | §9.1 |

So this is one data point and it is a good one: a real program, ported
faithfully, verified against the original, needing one missing feature
that was missing for no reason. It is not the claim that everything
ports — which is why there is a §9.

---

## 9. The second port: `sort`

`examples/sort/` is `LC_ALL=C sort` with no flags: read the files named
on the command line, or standard input when none are, sort the lines by
byte order, write them out. Checked against GNU `sort` the same way —
seven input shapes, named files, several at once, a missing file, and a
1.2 MB file, all compared byte-for-byte (`sort_agrees_with_gnu_sort`).

It was chosen because it needs every one of §6's four.

**It needed four library functions that did not exist**, and every one of
them was absent for the same reason: nothing had asked.

| Missing | Why nobody had noticed |
|---|---|
| `vec.set` | A vector could be `push`ed and `get` but never **written**. `push` and `get` were what the first caller wanted; a sort is the first caller that wanted the third |
| `vec.swap` | Expressible with the other two, and every sort writes it |
| `buffer.room` | `fs_read` writes *into* a slice you hand it, and every `std.buffer` operation took the bytes as an argument — the wrong direction when the filesystem is producing them |
| `buffer.filled` | The other half: a buffer cannot see a write it did not make, so the caller commits the count |

None is a design question. `vec.set` is three lines and `buffer.room` is
two. What is worth noticing is the *shape* of the gap: the library had
grown exactly the operations its existing callers needed, and the first
program with a different shape found four holes in an afternoon. That is
an argument for more ports rather than for more library review.

### 9.1 The heap, and what `fs_read` cannot tell you

Five owned resources, all on the heap: the text, two parallel runs saying
where each line is, and the permutation being sorted with its scratch.
`main` creates all five, lends them down, and destroys all five.

Under valgrind, sorting two files:

```
total heap usage: 8 allocs, 8 frees, 217,248 bytes allocated
in use at exit: 0 bytes in 0 blocks
ERROR SUMMARY: 0 errors from 0 contexts
```

Eight and eight — the five, plus the reallocation each growable one did
on the way. Nothing here is a runtime checking that; the balance is what
the type checker refused to compile without.

The awkward part is not ownership, it is that **`fs_read` cannot report
truncation**. It "fills as much of `into` as the file has and returns the
byte count" (`filesystem.md` §3), so a file that exactly fills the buffer
is indistinguishable from one that was cut short. There are no handles
and no way to ask a file's size, so `read_file` reads into 64 KiB, and if
the answer came back *equal* to the buffer it doubles and reads the whole
file again. A 1.2 MB file is therefore read six times.

> **Measured since, and sharper than this paragraph.**
> `file-handles.md` §1 traced it: six reads, but **2.75×** the file's
> bytes rather than six times them, because each attempt stops at its
> own capacity. The constant factor is under 3 at every size, which is a
> *weaker* argument than "six times" sounds.
>
> The real costs were the two this paragraph missed, and **both have
> since been fixed without handles**. The loop gave up at **8 MiB**, not
> the 16 MiB `sort.ls` claimed, so the example could not sort a file of
> 8,388,608 bytes or more (§1.1) — it now reaches 1 GiB, with a fixture
> past the old bound in the conformance suite. And giving up returned
> the same `-1` as a file that could not be opened (§1.2) — it now
> answers `-2` and exits 3.
>
> What survives is the part `sort.ls` cannot reach from inside itself:
> neither failure **prints** anything, because there is no standard
> error (`reach.md` §6). A script can tell them apart; a person cannot.

That is not a bug and it is the cost of `filesystem.md` §3's own
position: *"a file handle is a linear resource — it is precisely the
thing this language exists to track — and giving it a type means deciding
what `close` consumes... That is a milestone, not a paragraph."* The
milestone now has a program waiting for it.

### 9.2 Linearity at scale: the move loop

The shape that repeats everywhere:

```
out = buffer.push(heap, out, byte_of(c));
```

`std.buffer` and `std.vec` are move-based — every operation takes the
resource by value and hands it back — so a loop that fills a buffer
**moves it round and round**. Six sites in this program do it.

The honest verdict: it reads fine and it is noisy. `out = f(h, out, x)`
says exactly what happens and never lets you forget which value you have,
and it is three tokens longer than `f(&mut out, x)` every single time.
Nothing about it was hard. Nothing about it was pleasant either, and a
reader looking for the ergonomic cost of linearity should look here
rather than at the type signatures.

One thing it did make easy: the failure path. `read_file` returns
`(Buffer, int)` so the buffer comes back even when the read failed,
because a function that owns a resource and takes an error exit has to
say what happened to it. The tuple is not elegant; it is also not
something you can forget to write.

### 9.3 Rows at depth

`merge` takes five references and its row is `[]`. `read_file`'s is
`[heap, fs_read("")]`. `main`'s is `[]`, because it owns.

`reach.md`'s worry — that every helper which transitively does something
needs the plumbing — did not materialise here, and the reason is
specific rather than lucky: **this program's effects are concentrated at
its edges.** Reading and writing happen in four of the eight functions;
the sort itself touches nothing, so the other four declare `[]`:

```
read_stdin    [heap, io_read]
read_file     [heap, fs_read("")]
find_lines    [heap]
write_lines   [io_write]
before        []
merge         []
msort         []
main          []
```

Half and half, and the half that declares nothing is the half doing the
work the program is named after.

That is probably typical of programs shaped like this one and probably
not typical of everything. A program that logged inside its inner loop
would thread `io_write` through all eight, and the row would be right to
make that visible — but it would be eight annotations rather than four.

### 9.4 `borrow mut` in a program that needed it

This is the one that surprised me. The sort holds five things at once —
text, starts, lengths shared; order and scratch unique — and the checker
took it without complaint, because they are five *different* values and
`borrow mut` freezes only what it borrows.

The friction people report with lexical regions is about wanting to read
one field while writing another *of the same value*, and this program
never needed to. What it needed instead was five nested `borrow` blocks
in `main`, indented five deep, to hand those references to one call. That
is the real cost here and it is syntactic: the rule was never in the way,
the indentation was.

### 9.5 And the authority report

```
performs
    args
    err_write
    fs_read("")
    heap
    io_read
    io_write
never touches
    foreign code
```

`fs_read("")` is **unnarrowed**, and that is correct rather than sloppy:
this program reads paths its user supplies, so there is no prefix it
could commit to. Compare `examples/lines.ls`, which narrows to `/tmp`
because it chooses its own paths.

So the report distinguishes *a tool that reads what you tell it to* from
*a tool that reads somewhere specific*, and the difference is legible
without reading either program. That is the narrowing story doing the job
it was built for, on a program written to a specification that had never
heard of it.

---

## 7. Open

| Question | Why it waits |
|---|---|
| ~~A port with resources and depth~~ | **Done — §9**, and it found four missing library functions, confirmed the move loop's cost is noise rather than difficulty, and left `fs_read`'s truncation problem with a program waiting on it |
| File handles | §9.1. Reading a file of unknown size means reading it repeatedly, and `filesystem.md` §3 already says the fix is a milestone rather than a paragraph. There is now a program that pays for its absence |
| A `borrow` that takes several values | §9.4. Five nested blocks to hand five references to one call. The rule was never in the way; the indentation was |
| `base64 --wrap=N`, `-i` | Deliberately not ported. They are flag parsing, which is `ROADMAP`'s ordinary work, and adding them would have tested the flag parser rather than the language |
| An unsigned width | Not needed here, because base64 never sets the sign bit of an `int`. A port that hashes or checksums would need one within five lines (`defined-behaviour.md` §8) |

---

## 8. The suite

| Test | Shows |
|---|---|
| `base64_agrees_with_coreutils` | §1: the same bytes as `/usr/bin/base64`, both directions, twelve sizes, three malformed inputs, and a megabyte through a 64 KiB arena |
| `an_http_server_written_in_lex_sys_answers_a_real_request` | §5's bug, after the fix — it is the test that found it |

| `sort_agrees_with_gnu_sort` | §9: seven input shapes, named files, several at once, a missing file, and 1.2 MB past the first read — and, with no reference present, that the output is still a sorted permutation of the input |

| Program | Shows |
|---|---|
| `examples/base64/base64.ls` | The first port: bits, and a program that owns nothing |
| `examples/sort/sort.ls` | The second: five owned resources, the heap, and rows at depth |
