# Standard input

> **Status: settled.** The one I/O direction the language did not have.

---

## 1. What is missing

`examples/lines.ls` is the M3 acceptance criterion: a real command-line
tool that reads and writes files, counts and filters. It cannot be piped
into.

Nothing here can. A program can write to the console (`putchar`), read
and write files (`fs_read`, `fs_write`), and read its command line
(`arg`). It cannot read the one input every tool in a pipeline gets. So
`wordcount.ls` counts an *embedded* document, which is a demonstration
of counting rather than a program anyone would run.

---

## 2. It is not a seventh capability

The obvious move is a `Stdin` capability, an eighth field on `Split`,
and a break in every program that calls it. That is the wrong move, and
the language already says why.

`Fs` is **one** capability with **two** effect labels:

```
fn read [&f](fs: &f Fs(p), ...)  -> [fs_read(p)]  int
fn write[&f](fs: &f Fs(p), ...)  -> [fs_write(p)] int
```

The capability is what you *hold*. The labels are what you *did with
it*. Reading a file and writing one are the same authority used in two
directions, and the row says which — so a caller knows from the
signature, which is what `arguments.md` §2 means by the whole thing
being about **visibility** rather than containment.

The console is the same shape. `Io` is the capability; reading from it
and writing to it are two directions of one authority, and each gets a
label.

> `standard-error.md` applies this a third time and finds the rule holds
> without amendment: standard error is a third *stream* rather than a
> third direction, `err_write` is its label, and it is still not a new
> capability. §2.1 there says what that widens — a grant of `Io` reaches
> descriptor 2 now, where before it could not — and why the row is what
> keeps that honest.

### 2.1 Which means the existing label is renamed

`putchar`'s effect is spelled `io` today. Add reading and that becomes a
vocabulary with a hole in it: `[io]` would mean *write*, `[io_read]`
would mean read, and every reader would have to be told which way the
bare one points.

So the labels are **`io_read`** and **`io_write`**, and `putchar`'s row
changes from `[io]` to `[io_write]`.

This is not a drive-by rename. There is no way to add a read label
without deciding what the write label is called, and `[io]` alongside
`[io_read]` is a decision too — the worse one. Doing it now costs 155
occurrences across 53 files; doing it later costs more, because there
will be more programs by then, and the wart is in every signature that
prints in the meantime.

### 2.2 What `Io` discharges

Owning an `Io` outright discharges both labels, the way owning an `Fs`
discharges both of its. "Owning discharges, borrowing declares" is
unchanged: `main`'s row stays `[]`, and a function holding `&!i Io`
still writes down exactly what it did.

---

## 3. The operation

```
getchar[&i](io: &!i Io) -> [io_read] int
```

One byte in, mirroring `putchar`'s one byte out, behind the capability
that authorises it. `-1` at end of input.

### 3.1 Why `int` and `-1`

A byte is 0..255, so `-1` cannot be one and the sentinel is
unambiguous — which is exactly why C's `getchar` returns `int` and not
`char`.

It also matches `fs_read`, which returns `-1` when a file could not be
read: `filesystem.md` §2 calls that "an ordinary outcome, not a broken
promise", and the end of input is the same kind of thing.

An honest caveat: an enum — `End` and `Byte(byte)` — would be *better*.
A sentinel is a check you can forget; an exhaustive `match` is one you
cannot, and "a value the program decides what to do about" is
`sharing.md` §3's whole argument for `Gen`. The reason it is `int` here
is consistency with `fs_read` rather than conviction, and §6 keeps the
question open for both of them at once.

### 3.2 Why only one operation

There is no `read_line`, no `read_all`, no buffer-filling read.

`boxed-slices.md` §4 made this call already, for growing: *"there is no
`grow`, `push` or `realloc`; growing is allocate-copy-end, every part of
which was already expressible"*, so `examples/buffer/` writes it down
and the policy belongs to the program. A line is a policy too — where
it ends, what to do with a carriage return, what happens when it is
longer than a buffer — and none of those belong in a compiler.

`examples/tally.ls` is the demonstration: a real `wc` over standard
input, built from `getchar` and nothing else.

Writing it found two things worth having found, which is the argument
for building the example rather than asserting the feature:

* **The word-boundary policy was wrong twice.** The first versions
  omitted vertical tab and form feed. Nothing in the fixture contained
  either; running it against GNU `wc` over a real file did. A policy in
  the program is a policy you can get wrong — and also one you can fix
  without touching a compiler, which is the trade §3.2 is making.
* **It disagrees with `wc` in the C locale, on purpose.** Over this
  repository's README — 758 lines, 42158 bytes — lines and bytes agree
  exactly, and words agree under a UTF-8 locale. Under the C locale `wc`
  reports 98 fewer, because it decodes each multi-byte sequence and
  skips the ones the locale calls invalid: an em dash between two spaces
  is not a word to it, and is one to `tally`.

  That is `strings.md` §1 showing up in a program. A string here is
  **bytes, not an encoding**, so a run of non-blank bytes is a word
  whatever those bytes mean. Matching the C locale's answer would need a
  decoder, and nothing in this language has one.

---

## 4. What this is not

* **No promise about buffering.** `getchar` is libc's, so libc's
  buffering applies, and a byte costs a call — the same cost `putchar`
  has always had. A program that wants fewer calls reads into a buffer
  it owns, which is §3.2.
* **No seek, no rewind, no `isatty`.** Standard input is a stream of
  bytes that ends. Anything that asks where it is in a file is a
  question about a file, and files have `Fs`.
* **No line discipline.** Terminals do their own; this reads what
  arrives.
* **No second stream.** There is one standard input. Standard *error*
  is a separate missing thing and is not this document.

---

## 5. The harness learns `//~ STDIN`

A fixture that reads input needs input to be tested with, and every
harness here feeds a program nothing. So the directive vocabulary gains
one:

```
//~ STDIN  the text fed to the program
//~ STDOUT what it must print
//~ EXIT   the status it must exit with
```

It generalises: the accept walker, the example walker and any later
fixture get it at once, because they all read directives from the same
header.

---

## 6. Open

| Question | Why it waits |
|---|---|
| An enum instead of `-1`, for `getchar` **and** `fs_read` | §3.1. It is the better design and it is a change to two operations, which makes it its own slice rather than a rider on this one |
| A buffer-filling read | §3.2. A library first; a builtin only if the library proves it cannot be one |
| Standard error | A second stream, and a question about what `Io` is. Probably a third label |

---

## 7. The suite

| Fixture | Rule | § |
|---|---|---|
| `getchar_without_capability.ls` | Reading the console needs an `Io` | 2 |
| `io_read_undeclared.ls` | An effect performed is an effect declared | 2 |
| `io_write_does_not_cover_io_read.ls` | The two labels are distinct, and a row saying one does not permit the other | 2.1 |

The third is the one that matters. It is what makes the rename a real
distinction rather than a spelling: a function declaring `[io_write]`
may not call `getchar`, and the refusal names the label it is missing.

| Accepting | Shows |
|---|---|
| `stdin_roundtrip.ls` | `getchar` to end of input, a row carrying both labels, and the first fixture with a `//~ STDIN` |
| `examples/tally.ls` | A real `wc` over standard input: the program that could not be written before, and the one that found the two things in §3.2 |
