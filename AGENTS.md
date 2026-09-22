# Writing lex-sys

> The entry point this repository did not have. `docs/` is **90,000
> words across 42 documents** and none of them is a first page, so an
> agent asked to write lex-sys had the choice of reading all of it or
> guessing. This is the short version, and every rule in it is here
> because something in this repository got it wrong first.
>
> `lex-sys agent-guidelines` prints this file. The compiler carries it,
> so a checkout is not required to read it.
>
> **Every checked code block below is run by the test suite.** The ones
> marked `lex-sys` must compile; the ones marked `lex-sys-refused` must
> be refused, with the rule they name. A guideline that stops being true
> is a red build, not a stale paragraph.

---

## 0. The loop

```sh
lex-sys check src/*.ls --std               # does it type-check?
lex-sys check src/*.ls --std --output json # …and which rule, as data
lex-sys authority src/*.ls --std           # what can it reach?
lex-sys run src/*.ls --std                 # build and run in one step
```

`check` reports **every** independent refusal, not the first. On a
failure, read the `rule` field rather than the sentence: it is a stable
name, there are 52 of them, and `docs/agent-errors.md` is the contract.

**Do not regenerate a body because it was refused.** Every rule below
names what to change.

---

## 1. Ownership moves, and there is no `&mut self`

`std.buffer`, `std.vec` and `std.list` take their resource **by value**
and hand it back. A loop that fills one moves it round and round:

```lex-sys
import std.buffer;

fn collect[&h, &i](heap: &!h Heap, io: &!i Io) -> [heap, io_read] buffer.Buffer {
    var out = buffer.empty(heap, 64);
    var c = getchar(io);
    while c >= 0 {
        // The shape that repeats everywhere: take it, hand it back.
        out = buffer.push(heap, out, byte_of(c));
        c = getchar(io);
    }
    return out;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(ffi); release(fs); release(args);
    borrow mut heap as &!h in {
        borrow mut io as &!i in {
            let text = collect(h, i);
            buffer.drop(h, text);
        }
    }
    release(heap); release(io);
    return 0;
}
```

`out = f(h, out, x)` is three tokens longer than `f(&mut out, x)` every
single time, and it is the cost of linearity stated honestly
(`docs/porting.md` §9.2 — six sites in one program).

**A function that owns a resource and can fail must hand it back on the
failing path too.** `read_file` answers `(Buffer, int)` for exactly that
reason: a tuple is not elegant, and it is not something you can forget
to write.

---

## 2. A `res` value is consumed exactly once, on every path

This is the largest family of refusals in the suite after plain type
errors — 19 fixtures. Four ways to get it wrong:

```lex-sys-refused
//~ RULE linear-value-unconsumed

// Nothing consumes `heap` before the block ends.
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(io); release(ffi); release(fs); release(args);
    return 0;
}
```

The others: using one after it has moved (`linear-use-after-move`, 8
fixtures), consuming it in one branch and not another, and taking a
field out of it instead of taking the whole value apart.

**A capability is a `res` value like any other.** `main` owns what
`split` hands it and must `release` each one; a helper **borrows**.

---

## 3. Own or borrow, and the row follows

> Owning discharges. Borrowing declares.

`main` owns its capabilities, so its row is `[]` — that is not a gap,
it is the parameter list saying something stronger. A function handed
`&!i Io` says `[io_write]`, because that is what it did with it.

```lex-sys
import std.io;

// Borrowed, so the row is exact.
fn greet[&i](io: &!i Io) -> [io_write] int {
    return io.write_all(io, "hello\n");
}

// Owns, so the row is empty.
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(ffi); release(fs); release(heap); release(args);
    borrow mut io as &!i in { greet(i); }
    release(io);
    return 0;
}
```

### 3.1 Narrow the body, never widen the row

An effect row is **exact in both directions**: a label the body performs
must appear, and a label that appears must be performed.

```lex-sys-refused
//~ RULE effect-not-declared

import std.io;

fn greet[&i](io: &!i Io) -> [] int {
    return io.write_all(io, "hello\n");
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(ffi); release(fs); release(heap); release(args);
    borrow mut io as &!i in { greet(i); }
    release(io);
    return 0;
}
```

The fix is `[io_write]` here, because that is what the body does. Where
a row is wider than the body, **narrow the body** — do not widen the
row to make the checker stop. `lex-sys authority` prints the union of
what a program reaches, and a row that over-declares makes that report
a lie.

Labels: `io_read`, `io_write`, `err_write`, `fs_read(p)`, `fs_write(p)`,
`heap`, `args`, `ffi(lib)`.

---

## 4. A reference may not outlive its region

11 fixtures. A region is an arena, a `borrow` block, or a caller's
region parameter, and nothing that points into one may leave it:

```lex-sys-refused
//~ RULE reference-escapes-region

fn leak() -> [] &static [byte] {
    region a {
        let bytes = alloc_slice[a](4, byte_of(65));
        return bytes;
    }
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(io); release(ffi); release(fs); release(heap); release(args);
    return 0;
}
```

Take the region as a parameter instead — `fn f[&r](…) -> [] &r [byte]`
— so the caller decides how long it lives.

---

## 5. `res` and `val`, and what a container may hold

An arena and a boxed slice hold `val` data **only**: the fill is copied
into every element, and freeing the run is one `free` that runs nothing,
so a linear obligation put inside would be dropped rather than
discharged. 11 fixtures.

`std.vec` is `Vec[T: val]`, honestly. **`std.list` is the collection
that holds resources**, and the difference is the *shape* rather than
the generics — `docs/collections.md`.

---

## 6. Six things that cost this repository a compile each

Not rules so much as facts. Each was found by writing a program.

| | |
|---|---|
| **An arena is one 64 KiB chunk, and exhaustion traps** | Not an error — SIGILL. `examples/cut/`'s line buffer is 60,000 bytes because that is what fits beside a 1,025-byte bitmap. Use `std.buffer` on the heap when the size is not known |
| **`alloc_slice` already yields a reference** | A slice *is* a reference. `borrow mut` on one is a reference to a reference, and the refusal says `expected [byte], found &!a [byte]` |
| **A struct field cannot be `[T]`** | *"`[T]` has no size of its own."* Use a reference, or keep the slice beside the struct rather than in it |
| **There is no unary minus** | Write `0 - x`. The refusal for `-x` is a parse error and reads like a typo |
| **Six escapes, and no `\x` or `\u`** | `\n \r \t \\ \" \0`. A source file is already UTF-8, so `"café 日 😀"` needs none — `docs/strings.md` §8 |
| **`len` is a builtin and may not be redeclared** | Nor may any other prelude name. `std.buffer` calls its length `size` for this reason |

---

## 7. Use the library

`std.bytes`, `std.io`, `std.math`, `std.buffer`, `std.option`,
`std.result`, `std.list`, `std.vec`, `std.fmt`, `std.bignum`,
`std.utf8`. Pass `--std` and write the `import` — there is no prelude.

**A function earns its way into `std` by a program asking for it**, and
that is how the last nine arrived. If you need something that is not
there, write it in your program first; it moves into the library when a
*second* program needs it.

---

## 8. What this language does not have

Saying these plainly saves a cycle: no borrow checker (regions instead),
no traits, no closures or function values, no `comptime` beyond
`static` and constant folding, no async, no generics over effects, no
warnings — a diagnostic is a refusal.

`int` is 64-bit and **traps** on overflow rather than wrapping;
`wrapping_add` and its two siblings are how you ask for wraparound when
that is the intent.

---

## 9. Where to read more

| | |
|---|---|
| The rules of the type system | `docs/linearity-and-effects.md` |
| What a refusal means, as data | `docs/agent-errors.md` |
| What a program can reach, and how it is reported | `docs/reach.md`, `docs/authority.md` |
| Which collection holds a resource | `docs/collections.md` |
| Everything else | `docs/README.md` indexes all 42, with a one-line status each |
