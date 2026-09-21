# Examples

Programs meant to be **read**. Every one declares what it prints in its
own header, and `cargo test` walks this directory and checks it — so an
example that stops matching the language fails CI rather than quietly
rotting.

All of them build with `--std` available, which costs nothing: a program
that imports none of the library emits a byte-identical object either
way ([`standard-library.md`](../docs/standard-library.md) §5.2).

```sh
cargo run -p lex-sys -- run examples/tour.ls
```

| Example | What it is |
|---|---|
| [`hello.ls`](hello.ls) | The smallest program that says hello |
| [`tour.ls`](tour.ls) | One section per feature, in the order the milestones added them |
| [`pipeline.ls`](pipeline.ls) | M2 as *one* system rather than four features side by side |
| [`tree.ls`](tree.ls) | A binary search tree — why a language needs a heap at all |
| [`lines.ls`](lines.ls) | The M3 acceptance criterion: a real command-line tool |
| [`tally.ls`](tally.ls) | `wc` over standard input |
| [`wordcount.ls`](wordcount.ls) | `wc` over an embedded document, and the before-and-after for `std` |
| [`rational.ls`](rational.ls) | 250 lines of exact rational arithmetic; the M1 language still compiling unchanged |
| [`queue.ls`](queue.ls) | A work queue whose jobs **own** memory, held in a collection |
| [`pipeline_checks.ls`](pipeline_checks.ls) | A fallible pipeline: four exits, one `defer` |
| [`buffer/`](buffer/) | A growable byte buffer, written as a library |
| [`slab/`](slab/) | Shared ownership, as far as this language reaches |
| [`modular/`](modular/) | Two modules and a root |
| [`wordfreq/`](wordfreq/) | The capstone: three files, every capability doing real work |

---

## The tour

`tour.ls` is the shortest honest answer to "what can this language do".

```sh
cargo run -p lex-sys -- run examples/tour.ls
# M0: 7 5 3 1
# M1 bool: 1010010
# M1 struct: (3, 4) -> 25
# M1 enum: 0 12 20
# M1 generic: 5 3 z
# M2 linear: 4 7 9 5 6
# M2 borrow: 4 8 12
# M2 unique: 3 5 5
# M2 effects: 42
# M2 capability: 88
# M2 foreign: 7 9
# M2 arena: 1 4 9 -> 14
# M3 arithmetic: 6 1
# M3 slice: 3 1 4 1 5 -> 14
# M3 string: hello (5 bytes, e is 101)
# M3 file: on disk (7)
# M3 heap: 3 1 4 -> 8 (freed)
# M3 reading: 8 3 8 (kept)
# M3 args: 1 (named)
```

## Programs rather than a tour

### `pipeline.ls` — the thesis in one program

```sh
cargo run -p lex-sys -- run examples/pipeline.ls
# jobs: 4 done, 2 cancelled
# spent: 49 of 50
# headroom: 1
```

Admits a run of jobs against a budget. A job is a linear resource
finished exactly once — completed or cancelled, never both and never
neither; deciding which needs its cost, and reading a field of something
you own without spending it is a borrow; the running tally lives in an
arena; the overrun is computed by libc through a capability that names
libc and nothing else; and printing needs the console capability `main`
was handed. **Delete any one of those and it stops compiling.**

### `tree.ls` — why a heap exists

```sh
cargo run -p lex-sys -- run examples/tree.ls
# 1 3 4 5 7 8 9
# sum 37 count 7 depth 3
```

A binary search tree: its shape is decided at run time, its nodes
outlive the calls that made them, and the type is defined in terms of
itself. None of that fits in a block.

There is **no `free` in the file**, and every node is freed. `unbox` is
the only thing that ends a box, a box is `res`, and a node the program
forgot would be a compile error rather than a leak found next month.

### `lines.ls` — the M3 acceptance criterion

```sh
cargo run -p lex-sys -- run examples/lines.ls
# lines 6
# errors 2
# longest 15

lines /var/log/app.log        # or a path you name
```

Given a path it reads that file, counts and filters it, writes a report
and reads the report back; given nothing it lays down a sample and reads
that, which is how a tool with no input behaves anyway.

It is also where the design shows a sharp edge honestly. A capability is
narrowed to a **literal**, checked where it is written — so a tool that
reads a path the *user* chose cannot narrow to it, because there is no
literal to narrow to. `lines` therefore passes its `Fs` on unnarrowed,
and narrowing it to `/tmp` would not make a safer tool, it would make a
broken one.

What the types still buy at that width: `report`'s row says
`fs_read("")` and `fs_write("")`, so a reader knows it touches arbitrary
paths without opening the body. A tool that *did* know its directory
would narrow, and its row would say so instead. **The type tells the
truth either way.**

### `tally.ls` — `wc` over a pipe

```sh
printf 'the quick brown fox\njumps over\nthe lazy dog\n' \
    | cargo run -p lex-sys -- run examples/tally.ls
#      3      9     44
```

Worth reading for `count`'s row: `[io_read, io_write]` says in the
signature that the function consumes the program's input as well as
writing to the console, and a caller learns both without opening the
body.

### `wordcount.ls` — the before-and-after for `std`

```sh
cargo run -p lex-sys -- run examples/wordcount.ls --std
```

It used to open with `space`, `newline`, `write_all`, `print_nat` and
`is_blank` — five helpers that are not what the program is about, and
that 24 other files here each wrote out again. They are `std.io` and
`std.bytes` now.

`is_blank` moving out is the part worth noticing. `wordcount` counted
space and newline; `tally` counted six bytes; neither knew the other
disagreed. One definition, in one place, is most of what a standard
library is for.

### `queue.ls` — a collection of resources

```sh
cargo run -p lex-sys -- run examples/queue.ls --std
# compile 7
# link 4
# test 4
# rejected: 1 blank
# 3 jobs, 15 bytes
```

Jobs that **own** their storage, held in a `std.list`, ended exactly
once each. Three rules are visible and each is load-bearing: the list
moves jobs and never ends one, because a generic function does not know
what ending a `T` means; a job never finished does not compile; and the
tally beside it is a `Vec[int]` rather than a list, because an array
cannot hold a resource at all. [`collections.md`](../docs/collections.md).

### `pipeline_checks.ls` — four exits, one `defer`

```sh
cargo run -p lex-sys -- run examples/pipeline_checks.ls --std
# ok: fine
# empty
# bad prefix
# 3 checked
```

The program `docs/defer.md` §4 is about, and the reason it is an example
rather than a fixture is that the case for `defer` is not the code that
exists — it is the code nobody wrote. Without it, each of the four
bail-out paths repeats the buffer's drop, tangled into its own return
expression.

Both halves are checked rather than asserted: take the `defer` out and
the function stops compiling at the first early return, and with it in,
valgrind reports 4 allocs and 4 frees.

### `rational.ls` — the M1 language, still standing

250 lines of exact rational arithmetic, with a generic `Result[T]`
threaded through every fallible operation. It predates M2 and stays that
way on purpose.

## Libraries

### `newton.ls` — a method fixed point could not carry

Newton's method for √2, five steps, reporting the residual `|x² - 2|`
after each.

Read the numbers rather than the code. Q16.16 — the fixed point this
language forced before `float` — resolves 1.526e-5, and **step 3's
residual is already below that**, so the last three steps are invisible
to the representation the language used to have.

The honest detail is that the residual stops at 4.44e-16 rather than
zero: √2 is not representable in binary64, so the iteration settles on
the nearest value that is. A method that *converged* and a method that
reached the answer are different things, and this is where the
difference becomes visible.

Every number here is printed by `std.fmt.float_into` — the shortest
decimal that reads back to the same bits, written in lex-sys rather than
in the compiler (`float-printing.md`). An earlier version of this file
reported everything through `truncate` and a scale factor of 1e18,
because printing a float was still open; the difference is visible in
step 3, whose residual was `6007304882427` there and is
`6.007304882427178e-6` here. Three digits the scale factor was eating.

### `base64/` — a program that did not start here

GNU coreutils' `base64`, ported and checked against it: the conformance
suite pipes the same bytes through both binaries and compares, twelve
sizes, both directions, three malformed inputs and a megabyte.

It is the one program in this directory that had opinions before it
arrived — the 76-column wrapping, the padding, the exit status on bad
input — and it is worth reading for what it *did not* need. No new
capability, no library, no change to linearity or effect rows. What it
needed was the bit operators, which did not exist until it asked
(`docs/bitwise.md`).

It streams, and that is not style: an arena is one 64 KiB chunk and
standard input is not, so three-bytes-in-four-characters-out was the only
way to write it — which is also how the C writes it.

`docs/porting.md` is the report, including §6 on what one small port does
not establish.

### `sort/` — the port with resources in it

`LC_ALL=C sort`: the files named on the command line, or standard input
when none are, sorted by byte order. Checked against GNU `sort` the same
way `base64/` is checked against GNU `base64` — seven input shapes, named
files, several at once, a missing file, and 1.2 MB past the first read.

This is the one to read for **linearity in a program that has some**.
Five owned resources, all on the heap: the text, two parallel runs saying
where each line is, and the permutation being sorted with its scratch.
`main` creates all five, lends them down, and destroys all five.

The shape that repeats is `out = buffer.push(heap, out, byte)` — the
library is move-based, so a loop that fills a buffer moves it round and
round. `docs/porting.md` §9.2 is honest about what that costs: nothing
hard, and three tokens every time.

Also worth reading for the rows. Four of the eight functions declare
`[]`, and they are the four doing the sorting — the effects live at the
edges, which is §9.3.

### `serve/` — a REST endpoint over a real socket

The answer to *can this language do X?*, where X is the one everybody
asks. It binds a TCP port named on the command line, accepts one
connection, routes the request line and answers with JSON — `GET /health`
gets a 200, anything else gets a 404.

Eight `extern fn` declarations against libc and nothing else. No socket
type, no `Net` capability, no HTTP module. The test suite makes the
request from Rust over loopback, both routes, and checks that the
`Content-Length` it declares is the body it sends — which it is by
construction, because the header and the body are assembled into the same
slice.

Read `main` first: three capabilities destroyed on three lines, so a
server that cannot touch a file, allocate on the heap or print to the
console. Then read `serve`, which is the whole socket lifecycle with the
capability *lent* in — it can call libc and it can do nothing else, and
its row says so.

`docs/reach.md` is the argument the program is evidence for, including
what this **cannot** reach and why it is one sentence rather than a list.

### `buffer/` — growing, written out

```sh
cargo run -p lex-sys -- run examples/buffer/main.ls examples/buffer/buffer.ls
# counting: 1 4 9 16 25 36 49 64
```

Worth reading for `reserve`, which is the *entire* implementation of
"growing": a bigger box, a copy, and the old one ended. The language has
no `realloc` — which would also hide which of two very different things
happened — so the doubling policy is in this repository rather than in
the compiler, and the cost is countable: building 31 bytes from a
one-byte buffer is three allocations, which is what valgrind reports.

`std.buffer` is this, promoted.

### `slab/` — shared ownership, as far as it reaches

```sh
cargo run -p lex-sys -- run examples/slab/main.ls examples/slab/slab.ls
# live handle:  7
# after remove: missing
# new handle:   9
# old handle:   missing
```

A slab owns every value and hands out `Gen { index, generation }`
handles, which are two plain `int`s and copy like any other `val`. The
line worth watching is the last: a handle whose slot was removed comes
back `Missing`, which is a **value the program decides what to do about**
rather than a dangling pointer it gets no say in.

It is also the before-and-after for tuples and shadowing — writing it is
what found both gaps. `insert` and `look` each had to answer with a slab
*and* something else, so each declared a `res struct` that was not a
concept in the library; one function had to be split in two because a
struct pattern could not rename what it binds; and `run` had eight names
for one slab because a name could not be rebound. All three are gone, no
rule was weakened, and the object file did not change.

### `modular/` — two modules and a root

```sh
cargo run -p lex-sys -- run examples/modular/main.ls \
    examples/modular/counts.ls examples/modular/text.ls
# seen 3, total 60
# 60
```

`fmt.text` holds the console helpers, `fmt.counts` holds a tally and
imports `fmt.text`, and `main` imports both — one under its own name and
one renamed with `as`, because a qualifier is a name the importing file
chose rather than a path. It is also where `pub` earns its place:
`counts.step` is private, so it is an implementation detail rather than a
promise.

### `wordfreq/` — the capstone

```sh
cargo run -p lex-sys -- run examples/wordfreq/main.ls \
    examples/wordfreq/text.ls examples/wordfreq/counts.ls
# dog 1
# lazy 1
# over 1
# jumps 1
# fox 2
# brown 1
# quick 1
# the 3
```

Three files: `text.ls` holds byte helpers, `counts.ls` holds the tally,
`main.ls` is the program. Every capability is in it doing real work —
arguments, file IO, the heap, matching through references, slices — and
`bump` is worth reading in particular: it walks the tally through a
*unique* reference and increments a count in place, which is what
matching `&!c` is for.
