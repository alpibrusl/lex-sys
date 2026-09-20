# Command-line arguments

> **Status: settled, and built in the same change.** `filesystem.md` §6
> listed this as waiting on "a capability question answered first". This is
> that question and its answer.
>
> It is also what turns `examples/lines.ls` from a demonstration into a
> tool. That file currently writes its own input because it has no way to
> be told a path, which §5 there admits is "the one place the example is a
> demonstration rather than a tool".

---

## 1. The question

Arguments are the second thing the runtime hands a program, and the first
was a `World`. §8.2 is emphatic that there is exactly one of those and no
other way to obtain authority — so either arguments arrive through the
`World`, or §8.2 acquires an exception.

Three shapes were possible:

* **a second parameter to `main`** — `main(world: World, args: Args)`. It
  costs §8.2 its "exactly one" and buys nothing the other shapes do not;
* **ambient** — an `arg(n)` builtin reaching argv with no capability at
  all, because arguments are *data* rather than authority;
* **a capability in `Split`**, like every other thing the runtime provides.

This document takes the third, and §2 is why.

---

## 2. Reading argv is an effect

The tempting argument for ambient is that arguments grant no *power*. A
program that reads argv cannot damage anything by doing so: it learns
something, and learning is not authority. `Fs` still governs what it may
open, `Io` what it may print.

That argument is about containment, and it is correct as far as it goes.
The reason it does not decide the question is that **this language's
effect rows are about visibility, not only containment**:

> A function whose behaviour depends on the command line should say so in
> its type.

Ambient argv would mean a function eight frames down could branch on
`--force` with nothing in its signature, and nothing in any caller's
signature, saying that it does. That is exactly the property §7 refuses
for the console — `putchar` is not more dangerous than `arg`, it is more
*visible*, and that visibility is the whole point of an exact row.

So:

```
res Args                      // the command line
```

The sixth capability, carrying nothing, like `Io` and `Heap` — there is
one command line and no part of it to name, so there is nothing to narrow.
It is borrowed **shared**, like `Ffi` and `Fs` and unlike `Io` and `Heap`:
reading argv changes nothing, and two readers at once are the same as one.

```
let Split { io, ffi, fs, heap, args } = split(world);
```

Which breaks every program in the tree for the fourth time. The note in
§8.1 has said what that costs since `Ffi` was added, and the answer has
not changed: it is the price of there being no ambient authority, and it
is paid rather than avoided.

### 2.1 What this does not claim

`Args` is not a sandbox and does not pretend to be, in the same way
`filesystem.md` §2.1 says `Fs` is not. A program holding `Ffi("libc")` can
declare `extern fn getenv` and learn plenty about its environment without
asking anyone. What `Args` buys is that the *language's own* way of
reading the command line is one a reader can see in a signature.

---

## 3. Two operations

```
arg_count(a: &r Args) -> [args] int
arg(a: &r Args, n: int) -> [args] &static [byte]
```

`arg_count` is `argc` and `arg(a, 0)` is the program name, exactly as the
runtime was handed them. No translation: hiding `argv[0]` would be a
convenience the program cannot see through, and a tool that wants to print
its own name should be able to.

An index outside `0 .. arg_count` **traps**, like indexing past a slice.
It is the same kind of mistake and gets the same answer.

### 3.1 Why the region is `static`

An argument comes back as `&static [byte]`, which deserves a word because
`strings.md` §4 introduced `static` as "the region of data in the object
file" and argv is not in the object file.

What `Region::Static` *means* to the checker is **outlives everything**,
and that is precisely true of argv: the strings live in the memory the
process was started with, and they are alive from entry to exit. There is
no region they could fail to outlive. Reusing the region rather than
inventing a second one that behaves identically is the honest move, and
this paragraph is the documentation of it.

They are **shared**, never unique. Nothing in the program may write
through one — a program does not own its own command line, and `&!static`
is already refused everywhere else for the same reason.

### 3.2 No encoding, again

An argument is a run of bytes and claims nothing about what they mean,
exactly as `strings.md` §1 says of every other string here. The operating
system does not promise UTF-8 and this language will not pretend it does.

The bytes also carry **no NUL**: C hands over NUL-terminated strings and
`arg` computes the length, so what a program gets is the pointer-plus-
length pair every other string in this language is. The terminator is an
artifact of the C interface, not part of the value.

---

## 4. What this unlocks

`examples/lines.ls` becomes a real tool: given a path it reads that file,
and given nothing it falls back to the sample it writes itself — which is
how a command-line tool behaves anyway, and keeps the example runnable by
a test harness that passes no arguments.

M3's acceptance criterion asked for "a working CLI tool doing file IO and
parsing". It has had the file IO and the parsing since `filesystem.md`;
this is the "command-line" part of it.

---

## 5. What this does not add

* **No flag parsing.** `-v`, `--output=x`, clustering, `--` — all of it is
  a library over these two operations, and this language has no module
  system to put a library in.
* **No environment variables.** A second question with the same shape and
  a different answer available (`getenv` through `Ffi("libc")` already
  works). It deserves its own paragraph when someone needs it.
* **No standard input.** `Io` is the console capability and reading from
  it is a third operation; the file operations are whole-file and stdin is
  not a file. It waits for the same handle design `filesystem.md` §3
  defers.
* **No exit-code helpers.** `main` returns `int` and that is the exit
  status; there is nothing to add.

---

## 6. Open

| Question | Why it waits |
|---|---|
| Flag parsing | A library, and there is nowhere to put one |
| Environment variables | Same shape, different answer; nobody needs it yet |
| Standard input | Wants the file-handle design `filesystem.md` §3 defers |
| Arguments as an *iterator* | Wants a trait or a closure; the language has neither |

---

## 7. The must-reject suite

| Fixture | Rule | § |
|---|---|---|
| `args_without_capability.ls` | Reading the command line requires an `Args` | 2 |
| `args_effect_undeclared.ls` | A row that reads argv must declare `args` | 2 |
| `arg_written_through.ls` | An argument is shared; nothing writes through one | 3.1 |

And the accepting counterpart:

| Fixture | Shows |
|---|---|
| `arguments.ls` | `arg_count`, `arg(0)`, and a loop over the rest |

Plus two conformance tests, because both are things only a *running*
program with real arguments can show: the bytes a program is started with
are the bytes it reads, and an index past `arg_count` traps.
