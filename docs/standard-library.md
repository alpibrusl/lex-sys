# The standard library

> **Status: settled and built.** The thing `modules.md` was the
> precondition for.

---

## 1. What it is for

Across this repository, before this document, `print_nat` was written
out **25 times** byte for byte and `write_all` **19 times**. Not because
anyone wanted twenty-five copies, but because there was nowhere to put
one — `many-files.md` gave a program several files and one flat
namespace, and a *standard* library in a flat namespace would own
`open`, `push` and `total`, which programs here already use.

`modules.md` fixed the namespace. This is what goes in it.

---

## 2. How a program reaches it

Two ways, and the second is the point.

```sh
# Name the files, like any other module:
lex-sys build tool.ls std/io.ls std/bytes.ls

# Or ask for the whole library:
lex-sys build tool.ls --std
```

`--std` is **not** a search path. The library's source is compiled into
the `lex-sys` binary, so the flag adds no filesystem lookup, no manifest
and no build step — `modules.md` §6 promised none of those, and this
keeps that promise by not going near the disk at all.

It is opt-in, and deliberately so. There is no implicit prelude: a
program that says nothing gets nothing, and `import std.io;` is still
written where it is used. `--std` decides whether the *source* is
present, never whether a name is in scope.

### 2.1 What that couples

The library ships with the compiler, so its version is the compiler's
version. For a language at this stage that is the right trade — one
artifact, one thing to install, nothing to resolve — and it is the
decision to revisit first when a package story exists (§6).

---

## 3. The surface

Eight modules. The four below earned their place first; the four
collections that followed are `docs/collections.md`'s, and the short
version is that `std.list` holds resources, `std.vec` does not, and the
difference is the shape rather than the generics.

### 3.1 `std.bytes` — text, which here means bytes

`strings.md` §1: a string is **bytes, not an encoding**. So the text
module is a *byte* module, and every function in it is honest about
working on one byte or a run of them.

| | |
|---|---|
| `is_blank(c)` | The six bytes C's `isspace` calls space. `examples/tally.ls` got this wrong twice before it was written down once |
| `is_digit(c)`, `is_upper(c)`, `is_lower(c)`, `is_alpha(c)` | Classification, ASCII, no locale |
| `to_lower(c)`, `to_upper(c)` | ASCII case, and a no-op on anything else |
| `digit_of(c)` | The value of a digit byte, `-1` for anything else |
| `equal(a, b)` | Two slices, byte for byte |
| `starts_with(haystack, prefix)` | |
| `find(haystack, needle)` | The index, or `-1` |

`-1` rather than an enum, matching `fs_read` and `getchar`
(`standard-input.md` §3.1). The same caveat applies and the same
question is open.

### 3.2 `std.io` — the console

| | |
|---|---|
| `write_all(io, s)` | A slice of bytes, one `putchar` at a time |
| `print_nat(io, n)` | A non-negative integer |
| `print_int(io, n)` | Any integer, with the sign |
| `print_pad(io, n, width)` | Right-aligned in a field, for columns |
| `newline(io)`, `space(io)` | |
| `error_all(io, s)` | A slice, on the *other* stream — `[err_write]`, not `[io_write]` (`standard-error.md`) |

Every one of these takes an `&!i Io` and declares what it did with it —
`[io_write]` for all but the last, `[err_write]` for `error_all`, because
a library does not get to be quieter about its effects than a program
would be. That is `modules.md` §6 in practice: `pub` bought these
functions reachability and nothing else.

### 3.3 `std.math`

| | |
|---|---|
| `min(a, b)`, `max(a, b)`, `abs(n)` | |
| `gcd(a, b)`, `sign(n)` | `examples/rational.ls` wrote `gcd` for itself; now it need not |

`abs` on the most negative integer **traps**, because negating it
overflows and `defined-behaviour.md` §2.1 says an operation with no
right answer stops rather than inventing one. A library that quietly
returned the negative number would be exactly the silently-wrong answer
the language exists to refuse.

### 3.4 `std.buffer` — bytes, growable

A growable byte buffer: `Buffer`, `empty`, `reserve`, `push`, `append`,
`push_nat`, `size`, `write`, `clear` and `drop`.

`clear` arrived last and is the clearest case any of these has:
`examples/cut/` reads a line at a time and has to reuse its buffer, and
nothing here could move `used` back — `filled` only goes forward. The
alternative, `drop` plus `empty` on every line, is **13.0× slower**
(`docs/line-reading.md` §4). A gap rather than a convenience.

`size` rather than `len`, because `len` is a builtin and a program may
not redeclare one — which is the right refusal, and worth having hit
while writing the library rather than in someone else's code.

It is `res` and takes a `&!h Heap`, so it obeys every rule an ordinary
program's buffer would — it is `boxed-slices.md` §4's allocate-copy-end
written once instead of once per program. The doubling policy lives
here now, which is the *only* thing that changed: it is still policy,
still in a library, still not in the compiler.

`std.vec` is this with the element type lifted out, and it stays a
separate module rather than replacing this one: a byte buffer's
`append` and `push_nat` are about bytes, and a `Vec[byte]` that had
them would be a byte buffer wearing a type parameter.

### 3.5 The collections

`std.option`, `std.result`, `std.list` and `std.vec` —
`docs/collections.md`. What is worth carrying back here is that three
of the four needed **no language change**: a generic container has
worked at both modes since M2, and what was missing was a library that
said so. The fourth needed a bound on a type declaration, because
`res struct Vec[T: val]` is two modes about two different things.

### 3.6 `std.fmt` and `std.bignum` — printing a float

```
fmt.float_into(out, x) -> int       // bytes written, or -1 if `out` is short
```

The shortest decimal that reads back to the same bits — `0.1` as `1e-1`,
not as `0.1000000000000000055511151231257827` and not as `0.100000`.
`docs/float-printing.md` is the whole story; two things belong here.

**It is library code, and that is the interesting part.** Float printing
is the routine every other language keeps in its runtime, in C or Rust,
a thousand lines behind whatever interface it chose. Here it is
`std/fmt.ls`, written in lex-sys, with an effect row of `[]` and no
`Heap` — the working numbers live in a `region`. The compiler's entire
contribution is `bits_of`, a bitcast. Anything `float_into` does, a
program could have done.

**`std.bignum` is underneath, and has no division.** Exact integers to
1080 bits, base-2³² limbs in a fixed-length `[int]`, in place and
allocation-free. It is ninety lines rather than four hundred because the
one quotient the printer needs is a single digit, and nine subtractions
settle it.

Neither module is a general-purpose facility and neither pretends to be:
`std.bignum` is as wide as `std.fmt` needs, and `std.fmt` prints one
type. §4's "no allocation-free string formatting" still stands for
everything else.

---

## 4. What is deliberately not in it

* **No `std.slab`.** `examples/slab/` stays an example. `sharing.md`
  §3 is explicit that a generational handle is an escape hatch and
  "neither is reached for by default" — putting it in `std` would say
  the opposite.
* ~~**No `Option` or `Result`.**~~ **Both are in**, along with
  `std.list` and `std.vec` — see `docs/collections.md`. The reason first
  given here was wrong twice over, and the second wrong reason was the
  correction to the first.

  It said they "want generics over a mode", which the language does not
  have. It does, and has since M2 (`mode-polymorphism.md` §1); that
  claim was taken from §12's open list rather than from a test.

  The correction then said a `Vec[T]` over a resource type **is**
  expressible. It is not. A `Vec` keeps its elements in a boxed slice,
  and `collections.md` §2 gives two independent reasons a boxed slice
  holds `val` data only — the fill is copied into every element, and
  freeing the run is one `free` that *runs nothing*. `std.vec` is
  therefore `[T: val]`, honestly, and `std.list` is the collection that
  holds resources. The difference is the **shape**, which is what both
  earlier answers missed by looking at the generics.
* **No allocation-free string formatting**, with one exception that
  proves the rule. `print_*` writes to the console; formatting *into* a
  buffer is a second surface and wants §6's open question about writers
  answered first. `fmt.float_into` (§3.6) does write into a caller's
  `[byte]`, because printing a float has no second way to do it — and
  it is one function for one type, not the surface.
* **No implicit prelude.** §2.

---

## 5. What building it found

Three things, and the second is a language bug this slice fixed.

1. **A library must declare everything a program declares.** Every
   function here has an exact effect row and every capability is a
   parameter. Nothing was easier to write because it was "the standard
   library", which is what `modules.md` §6 predicted and is worth
   having confirmed.
2. **`--std` with no `import` must stay silent — and it did not.** This
   section first said "a declaration nobody calls costs nothing: emission
   only reaches what is called". That was written from the comment above
   the code, which said the same thing, and both were **wrong**. A
   program calling none of the library got a 6720-byte object against
   1048 without it.

   Emission seeded from *every non-generic function*, which is
   indistinguishable from reachability while every function in a program
   is one somebody wrote. A standard library is the first time it is not.

   Fixed rather than documented around, because a library you cannot
   afford to link is not standard. The two passes now have one job each:
   **checking is total** — every function, once, generics with rigid
   parameters — and **emission starts at `main`**. With no `main` there
   is no program, only declarations, so every non-generic function is a
   root instead; that is the binary-versus-library distinction every
   toolchain draws, drawn from the one fact available here.

   The fix is not about `std`. Every program gets it.

   It also made checking run in source order, which surfaced a
   **must-reject fixture that was passing for the wrong reason**:
   `effect_not_propagated.ls` had `putchar(33)`, missing the `Io`, and
   was refused for that rather than for the effect rule it tests. A
   fixture passing for the wrong reason deserves more attention than one
   failing, and this is the second time a rule's real test turned out to
   be somewhere other than where it was written down.
3. **Byte classification is where programs quietly disagree.**
   `tally.ls` and `wordcount.ls` each had their own idea of a word
   boundary and they were not the same. One definition, in one place,
   is most of what a standard library is *for*.

   > **Corrected (#57).** "One definition, in one place" was the plan,
   > not the outcome. `wordcount.ls` moved to `std.bytes`; **`tally.ls`
   > did not** — it kept a private `is_blank`, byte-identical, under a
   > comment arguing that a word boundary is a policy belonging in the
   > program. That argument was against putting it in the *compiler*,
   > and it was written before there was a library to be the third
   > option. So there were still two definitions for a year. They
   > agreed, and nothing checked that they did — which is the failure
   > mode this entry is about, surviving inside the entry that named it.
   > `tally.ls` imports `std.bytes` now.

---

## 6. Open

| Question | Why it waits |
|---|---|
| A writer abstraction — format into a buffer or a file, not just the console | Wants something like a trait, and there are none |
| A line reader | **Answered, no** — `line-reading.md`. Five programs call `getchar` and one reads a line at a time; the bar is two. The row that asked for it named a second program that keeps nothing at all, which is what §5's rule costs when the count is taken from memory rather than by reading |
| `split` returning a collection | `utf8.md` §1 verified `vec.Vec[&t [byte]]` compiles, so this is writable. `bytes.field` covers the case `examples/cut/` had without allocating, and nothing has yet needed the whole list at once — which is the bar the other four cleared |
| `Option` / `Result` over `res` types | Mode polymorphism, §12 |
| Versioning the library separately from the compiler | §2.1. A package story, which is `modules.md` §7's open question too |
| An enum instead of `-1` | `standard-input.md` §6, now for a third set of operations |
| `error_nat`, and what goes in front of a diagnostic | `standard-error.md` §3.2 and §8. A number on that stream wants `std.buffer` and one write, which is writable today; the prefix — `sort:`, `cut:` — is written out at eleven call sites, and what stops it being a function is where the name comes from, since `arg(g, 0)` needs a capability the failure site may have released |

---

## 7. The suite

The library is checked the way a program is: it compiles, its examples
run, and its rules have fixtures.

| Test | Shows |
|---|---|
| `the_standard_library_compiles_on_its_own` | Every module of `std/` type-checks together, without a program |
| `std_is_available_behind_a_flag` | `--std` compiles a program that imports it, with no file named |
| `std_declarations_cost_nothing_unless_called` | §5.2: a program built with `--std` and one built without it emit **byte-identical** object files. This was false when it was first written down, which is why it is a test |
| `abs_of_the_most_negative_integer_traps` | §3.3 |
| `examples/wordcount.ls` | Rewritten on `std` — five helpers gone — and prints exactly what it printed before |
| `examples/queue.ls` | The collections at work: jobs that own memory, held in a `List`, ended exactly once each |
| `printing_preserves_every_identity_and_is_idempotent` | `std/` walks with everything else: the library is code and gets the same contract |

Every example is now built with `--std` passed unconditionally, which is
only safe because of §5.2. That is the property being relied on rather
than merely asserted.
