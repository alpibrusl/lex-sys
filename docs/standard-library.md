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

Four modules. Each one earns its place below.

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

Every one of these takes an `&!i Io` and declares `[io_write]`, because
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

### 3.4 `std.buffer` — the one data structure

A growable byte buffer: `Buffer`, `empty`, `reserve`, `push`, `append`,
`push_nat`, `size`, `write` and `drop`.

`size` rather than `len`, because `len` is a builtin and a program may
not redeclare one — which is the right refusal, and worth having hit
while writing the library rather than in someone else's code.

It is `res` and takes a `&!h Heap`, so it obeys every rule an ordinary
program's buffer would — it is `boxed-slices.md` §4's allocate-copy-end
written once instead of once per program. The doubling policy lives
here now, which is the *only* thing that changed: it is still policy,
still in a library, still not in the compiler.

---

## 4. What is deliberately not in it

* **No `std.slab`.** `examples/slab/` stays an example. `sharing.md`
  §3 is explicit that a generational handle is an escape hatch and
  "neither is reached for by default" — putting it in `std` would say
  the opposite.
* **No `Option` or `Result`** — *and the reason given here was wrong.*
  This said they "want generics over a mode", which the language does
  not have. It does: `docs/mode-polymorphism.md` §1 shows a generic
  container used at a resource type and a copyable one in the same
  program, and it has worked since M2. The claim was taken from §12's
  open list rather than from a test.

  What they actually want is a place to put the value that is **not**
  returned: `unwrap_or` needs `T: val`, and a resource version needs a
  different signature. That is a library design question, and now that a
  bound can say which version is which, an ordinary one.
* **No collections beyond a byte buffer.** Same correction: a `Vec[T]`
  over a resource type is expressible. What a byte buffer does not need
  is a decision about what its emptying and copying operations mean for
  a linear element, which is the design that has not been done.
* **No allocation-free string formatting.** `print_*` writes to the
  console. Formatting *into* a buffer is a second surface and wants
  §6's open question about writers answered first.
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

---

## 6. Open

| Question | Why it waits |
|---|---|
| A writer abstraction — format into a buffer or a file, not just the console | Wants something like a trait, and there are none |
| `Option` / `Result` over `res` types | Mode polymorphism, §12 |
| Versioning the library separately from the compiler | §2.1. A package story, which is `modules.md` §7's open question too |
| An enum instead of `-1` | `standard-input.md` §6, now for a third set of operations |

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
| `printing_preserves_every_identity_and_is_idempotent` | `std/` walks with everything else: the library is code and gets the same contract |

Every example is now built with `--std` passed unconditionally, which is
only safe because of §5.2. That is the property being relied on rather
than merely asserted.
