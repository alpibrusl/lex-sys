# The filesystem

> **Status: settled, and built.** Shorter than
> `strings.md` because most of it is an *application* of decisions already
> made: `Fs(prefix)` narrows by §7.4's prefix extension exactly as
> `Ffi(library)` does, and the file operations reach libc the way `putchar`
> and the arena already do. Two things here are genuinely new, and §2 and §4
> are those two.
>
> This is the last mile to M3's acceptance criterion
> ([#1](https://github.com/alpibrusl/lex-sys/issues/1)): a real CLI tool
> doing file IO and parsing.

---

## 1. `Fs(prefix)` is a capability carrying a path

`linearity-and-effects.md` §8.1 listed it, and §7.4's worked example is
literally this one:

```
let logs = narrow(fs, "/var/log/app");   // Fs("/var") -> Fs("/var/log/app")
```

So it is the machinery `Ffi` already uses, pointed at a different kind of
name. `split` hands out `Fs("")` — the unnarrowed root, authority over
nothing until it names somewhere — and narrowing is prefix extension, one
way only. A program cannot grant itself what it was not given.

The effect labels carry the prefix too: `fs_read("/var/log")` and
`fs_write("/tmp")`. A function's row therefore says which part of the
filesystem it can touch, and a caller reads that without opening the body.

### 1.1 A path prefix is not a byte prefix

One place `Fs` is *not* the same as `Ffi`, found while building it and
worth stating rather than leaving implicit.

`libc` is a byte prefix of `libcrypto`, and that is the right answer for a
library name: attenuation there is textual because a library name has no
structure. A path does have structure. `/tmp` is a byte prefix of
`/tmpevil`, and `/tmpevil` is not in `/tmp` — it is the directory next
door, and a purely textual check would hand it over.

> **A path prefix extends at a `/`, or not at all.** `target` is inside
> `prefix` when it starts with those bytes *and* either is exactly the
> prefix, or continues from it at a separator.

Exactly the prefix is allowed on purpose: `Fs("/tmp/app.log")` naming one
file is an ordinary and useful point in the lattice, and a rule that
refused it would make single-file authority unexpressible.

The empty prefix contains everything, which is what makes the capability
`split` hands out the root rather than a special case. The rule is checked
twice, because the two halves are known at different times: `narrow`
checks it at compile time against the literal, and the operations check it
at run time against the path (§4).

---

## 2. Why the operations are builtins and not `extern fn`

This is the first of the two new decisions, and it is the one that decides
whether `Fs` means anything at all.

> **File operations reach libc from the backend, the way `putchar`,
> `malloc` and `free` already do. They are not `extern fn` declarations.**

If they were, they would be gated by `Ffi("libc")` — and then holding the
*FFI* capability would let a program open any path, with `Fs` contributing
nothing. The authority that guards the filesystem has to be the one that
names the filesystem.

### 2.1 What `Fs` does not do

**`Fs` is not a sandbox, and this document will not pretend otherwise.**

A program that narrows its `Ffi` to `"libc"` can declare
`extern fn open(...)` and reach any path it likes. Nothing in the type
system stops that, because §8.4 deliberately makes C reachable and C has
the whole filesystem in it.

What the design actually claims is narrower and still worth having:

* a program's authority is **visible in its types**. Reaching the
  filesystem through the language costs an `Fs` narrowed to a prefix;
  reaching it around the language costs an `Ffi("libc")` and an `extern`
  declaration, and both are in the signature of every frame that carries
  them;
* `main` chooses. It is handed one `World`, and what it narrows and what it
  releases is the whole story — a `main` that releases its `ffi` and
  narrows its `fs` to `/tmp` has given the rest of the program exactly
  that, and the rest of the program cannot widen it.

Containment against a *hostile* program is a sandbox's job — `lex-os` runs
one, and this language is what runs inside it. Containment against an
*honest* program's mistakes is what types do, and that is what this is.

---

## 3. Two operations, whole-file

```
fs_read(fs: &f Fs(p), path: &a [byte], into: &!b [byte]) -> [fs_read(p)] int
fs_write(fs: &f Fs(p), path: &a [byte], bytes: &c [byte]) -> [fs_write(p)] int
```

The capability is borrowed *shared*, like `Ffi` and unlike `Io`: holding it
is a key, not a conversation, and two frames holding the same key at once
changes nothing about what either may open. `into` is unique because
`fs_read` writes through it; `bytes` and `path` are shared because nothing
here writes through them.

`fs_read` fills as much of `into` as the file has and returns the byte
count; `fs_write` writes the whole slice and returns what it wrote. Both
return `-1` on failure rather than trapping: a missing file is an ordinary
outcome a program should handle, not a bug in the program.

No handles, no streaming, no seek. A file handle is a linear resource — it
is precisely the thing this language exists to track — and giving it a type
means deciding what `close` consumes, what a half-read file is, and what
happens to a handle at the end of a region. That is a milestone, not a
paragraph, and whole-file operations are what M3 needs to read its own
source and write its own output.

---

## 4. The path is checked at runtime, and `..` is refused

The second new decision, and the one with the sharp edge.

The prefix lives in the *type* and is therefore known at compile time. The
path is a runtime slice — it has to be, or a program could not open a file
named on its command line. So:

> **The operation emits a check that the path starts with the capability's
> prefix, and traps if it does not.**

A trap rather than `-1`, because a path outside the granted prefix is not a
missing file: it is a program doing something its type said it would not.
That is the same distinction `defined-behaviour.md` draws everywhere —
`-1` for an outcome, a trap for a broken promise.

### 4.1 `..` is refused rather than normalised

A prefix check on bytes is defeated by `/tmp/../etc/passwd`. There are two
honest responses and one dishonest one:

* **normalise the path** before checking — which is a security function
  with a long history of being got wrong, and would need its own design,
  its own fixtures and a decision about symlinks;
* **refuse any path containing `..`** — a trap, like the prefix check;
* pretend the prefix check is sufficient, which is the dishonest one.

M3 takes the second. It is a real restriction and it is stated rather than
hidden: a program that needs `..` is a program that needs path
normalisation, and path normalisation arrives with a design document of its
own.

---

## 5. What this unblocks

M3's acceptance criterion. [`examples/lines.ls`](../examples/lines.ls)
writes a log, reads it back off disk, counts and filters it, writes a
report, and reads the report back to print it — four file operations, one
capability narrowed once in `main`, and a row on every frame that carries
it. The bytes come from disk; everything done to them is the slice
machinery `wordcount.ls` already used.

There are no command-line arguments yet (§6), so the tool lays down its own
input rather than being handed a path. That is the one place the example is
a demonstration rather than a tool, and it is a missing *runtime* feature
rather than a missing language one.

---

## 6. Open

| Question | Why it waits |
|---|---|
| File handles as linear resources | The obvious next step, and the one this language is *for*. Needs `close`, partial reads, and what a region's end does to a handle |
| Path normalisation | A security function; needs symlinks decided too (§4.1) |
| Command-line arguments | `main` takes a `World` and nothing else today; arguments are another thing the runtime hands over, and they need a capability question answered first |
| Directory listing | Another operation, and a second shape of result |
| `-1` versus a result type | `Result[T]` exists (`rational.ls` uses one). Whether the filesystem should return one is a library-shape question |

---

## 7. The must-reject suite

| Fixture | Rule | § |
|---|---|---|
| `fs_without_capability.ls` | Reading a file requires an `Fs` | 1 |
| `fs_widened.ls` | `Fs("/tmp/a")` cannot become `Fs("/tmp")` | 1 |
| `fs_sibling_prefix.ls` | `Fs("/tmp")` cannot become `Fs("/tmpevil")` | 1.1 |
| `fs_effect_undeclared.ls` | A row must name the prefix it reads | 1 |

And the accepting counterparts:

| Fixture | Shows |
|---|---|
| `file_roundtrip.ls` | Write a file, read it back, compare the bytes |
| `fs_narrowed.ls` | A capability narrowed once, threaded down, used at the bottom |

Plus four conformance tests, because these are runtime rules and a fixture
that only *compiles* would not reach them:

| Test | Shows | § |
|---|---|---|
| `a_path_outside_the_granted_prefix_traps` | The prefix is enforced on the value, not just on the type | 4 |
| `a_path_containing_dot_dot_traps` | `..` is refused rather than normalised | 4.1 |
| `a_sibling_of_the_granted_directory_traps` | §1.1 again, on the path this time | 1.1 |
| `a_missing_file_is_minus_one_rather_than_a_trap` | An outcome is not a broken promise | 3 |
