# File handles

> **Status: built (#66).** §2.1, §4.1, §4.2 and §6 are what building it
> settled; everything above them is the design as it stood, which was
> right about the shape, wrong about one signature, and silent about
> what three new names cost.
>
> `filesystem.md` §3 called this *"a milestone, not a paragraph"* and
> named three questions it would have to answer. **Two of the three are
> already answered** by machinery that exists and is tested — this
> document's first job is to retire them. The third is real, and it is
> the same question `standard-input.md` §3.1 defers, so settling it here
> settles both.
>
> The fourth question — the one §3 did not name — is the interesting
> one, and it is about the authority report rather than about linearity.

---

## 1. What the absence costs, measured

`examples/sort/` has to read a file of unknown size through `fs_read`,
which *"fills as much of `into` as the file has and returns the byte
count"*. A file that exactly fills the buffer is indistinguishable from
one that was cut short, so `read_file` reads into 64 KiB and doubles
until the answer comes back short.

Under `strace`, on a 1,160,000-byte file:

| | |
|---|---|
| reads of the file | **6** |
| bytes read | **3,191,616** |
| as a multiple of the file | **2.75×** |

`porting.md` §9.1 says *"a 1.2 MB file is therefore read six times"*.
Six reads is right; **six times the I/O is not**, and the sentence
invites that reading. Each attempt stops at its own capacity, so the
total is the sum of the capacities, not six copies:

| file | reads | bytes read | ratio |
|---|---|---|---|
| 100,000 | 2 | 165,536 | 1.66× |
| 1,160,000 | 6 | 3,191,616 | 2.75× |
| 3,000,000 | 7 | 7,128,768 | 2.38× |
| 8,388,607 | 8 | 16,711,679 | 1.99× |

The ratio is bounded by 3 and has no trend — it is worst just after a
doubling and best just before one. So the cost is **a constant factor
under 3, not a factor of six**, which is a weaker argument for handles
than §9.1 was making. The strong arguments are the next two.

> **Measured again after the port (#67): `porting.md` §10.** Every one
> of those ratios is now **1.00×**, and the time moved **7%** on an
> 8.4 MB file. Three times less I/O bought single digits, because a sort
> spends its time sorting — which is `bulk-io.md` §4.1's correction
> arriving from the reading side. The syscall count went the other way
> (6 reads became 19 on the 1.16 MB file) and costs nothing measurable:
> 4 KiB, 64 KiB and 1 MiB chunks are within 1.3% of each other.
>
> So this section's own verdict stands and sharpens. The performance
> argument for handles was the weak one; what they were worth is in
> §10.3 — a ceiling deleted rather than raised, a status GNU does not
> have deleted with it, and an `errno` finally in reach.

### 1.1 The ceiling was 8 MiB, and the comment said 16

`sort.ls` reads:

> *"Eight doublings from 64 KiB reaches 16 MiB, which is where this
> gives up rather than growing without bound."*

It does not. Eight *attempts* starting at 64 KiB reach a largest
capacity of 8 MiB, and the file has to come back **strictly** shorter
than the capacity to be believed. Measured:

| | |
|---|---|
| 8,388,607 bytes | exit 0 |
| 8,388,608 bytes | **exit 2** |
| 9,000,000 bytes | **exit 2** |

So `examples/sort/` could not sort a file of 8 MiB or more. Not a design
decision — an off-by-one in a comment that nothing checked, in the one
place a reader would look to find the limit.

**Fixed, and it did not need handles.** Fifteen attempts reach 1 GiB,
which is past where a sort holding the whole file in memory has worse
problems; a conformance fixture now sorts 9 MB and fails on the old
bound. A ceiling still exists rather than growing until something gives,
because `heap.md` says a failed allocation **traps** — without one, a
file bigger than memory would abort instead of saying so.

### 1.2 Too big was indistinguishable from missing

Worse than the volume. After eight attempts `read_file` returns `-1`,
which is the same value it returns when the file could not be opened at
all. Both exit 2, and both print nothing:

```
$ sort just_over.txt   ; echo $?     # 8 MiB file that exists and is readable
2
$ sort /nope/missing.txt ; echo $?
2
```

Silent because there was no standard error to be loud on. This document
filed that under `reach.md` §6, and **`reach.md` never named it** — not
in §6, not anywhere; the citation pointed at a table of five rows, none
of which was this. `standard-error.md` §1 is where the gap actually got
written down, and closed. So §9.1's finding is sharper than §9.1 put it: `fs_read` cannot
report truncation, **and the workaround cannot report its own failure
either**. A program that hit the ceiling looked exactly like a typo in a
filename.

**Half-fixed, and the other half is why this document exists.**
`read_file` now answers `-2` for "larger than I can grow to hold"
against `-1` for "could not read it", and `main` exits **3** rather than
folding it into the **2** GNU uses for a file that is not there. So the
two are distinguishable *to a script*.

They were still not distinguishable to a **person**, because neither
printed anything, and no amount of work inside `sort.ls` reached that:
an exit status was the whole vocabulary this program had.

That is the argument for handles. Not the 2.75×.

> **Fixed, and again it did not need handles** (`standard-error.md`).
> `sort` now says `sort: cannot read: <path>` and
> `sort: file too large: <path>`, on a stream a redirect of the output
> does not capture. What is still missing is the *reason* a read failed
> — GNU names the errno string and `fs_read` answers a bare `-1` — which
> is §3's `Failed(int)` and belongs to this document. Two of the three
> things §1 was written to motivate have now been fixed without the
> design it motivates, which is worth noticing twice.

---

## 2. Two of §3's three questions are already answered

§3 said giving a handle a type *"means deciding what `close` consumes,
what a half-read file is, and what happens to a handle at the end of a
region."*

**What happens at the end of a region: nothing new.** A linear value
that reaches the end of a block is already a compile error, region or
not:

```
region a {
    let f = F { fd: 3 };
}
// error: `f` is still live at the end of this block; nothing consumes it
```

The worry was that a region's bulk free would reclaim the memory and
leak the descriptor. It cannot, because the checker will not let a `res`
value die by scope exit in the first place. A region frees *arena*
allocations; a linear value has to be consumed by name wherever it
lives.

**What `close` consumes: the handle, and the checker already enforces
it.** That is what `res` means, and `sort.ls`'s five owned resources
already demonstrate it at scale (`porting.md` §9.1: eight allocs, eight
frees, nothing counting them at runtime).

There is a third thing §3 did not ask about and might have: whether a
handle can live inside a `Result`, since `open` can fail and handing
back a handle *and* an error means the caller holds a handle it must
consume even on the failing path. It can, and `std.result`'s header
already says so in as many words:

> *"`Result[Ticket, int]` is a resource in the `Ok` arm and an ordinary
> integer in the `Err` arm, and the **type** is a resource either way."*

Verified rather than taken on trust — linearity reaches through a
generic enum:

```
let m = Maybe::Some(make_a_resource());
return 0;
// error: `m` is still live here; a `res` value must be consumed on every path
```

So the shape is available today:

```
open_read[&c, &a](fs: &c Fs(p), path: &a [byte]) -> [fs_read(p)] Opened
file_close(file: File) -> [] int
```

(`Fs(p)` is `filesystem.md` §3's own schema notation for "whatever
prefix the capability carries", not literal source — real code spells a
concrete one, as `sort.ls` does with `Fs("")`.)

### 2.1 That signature cannot be written, and the reason is structural

`Result[T, E]` is `std.result` — a **library** type, declared in
`std/result.ls` and reachable only with `--std`. A builtin's signature
is fixed in the compiler, in terms of the prelude, and has to type-check
in a program compiled without the standard library at all. So
`open_read` cannot return a `Result`, and no amount of care with the
declaration changes that: the type does not exist when the signature
does.

This is not a wrinkle in the design so much as the design meeting a rule
`standard-library.md` set and this document forgot — **`std` is opt-in,
never a prelude.** A builtin that needed `std` would make it one.

So `open_read` answers a *prelude* enum with the same two arms:

```
enum Opened {
    Ok(File),
    Failed(int)   // errno
}
```

and `std.fs` is free to offer `into_result(o: Opened) -> [] Result[File, int]`
for a program that wants the library shape. The linearity argument §2
verified is unaffected — it was about a `res` payload surviving a
generic enum, and `Opened` is not generic, which is strictly easier.

The cost is one name. What it buys is that a program can open a file
without `--std`, which is the same property every other builtin has.

**The milestone is smaller than §3 advertised.** What is left is one
question about reading, and one about the effect row.

---

## 3. What a half-read file is

This is §3's third question and `standard-input.md` §3.1's open row, and
they are the same question: a read has **three** outcomes and `int` has
been carrying two.

`getchar` answers `-1` at end of input. `fs_read` answers `-1` on
failure. Those are different meanings on the same sentinel, in the same
language, and a bulk read needs both at once plus the ordinary one.

The rule:

```
file_read[&f, &b](file: &!f File, into: &!b [byte]) -> [file_read] Read
```

```
enum Read {
    Got(int),   // 1..=len(into) bytes, at the front of `into`
    End,        // the file is over; `into` is untouched
    Failed(int) // errno; `into` is unspecified
}
```

Three constructors because there are three outcomes, and an enum rather
than a sentinel because **a sentinel is how `getchar` and `fs_read`
came to disagree**. `Got(0)` is not reachable: a read that returns
nothing and is not at the end is `End` or `Failed`, never a zero.

The row was `[?]` when this was written; §4.1 settled it as `file_read`,
which is also the builtin's name for the reason `fs_read` is both.

`match` is free here in the sense that matters. Checked rather than
assumed — a three-constructor `Read` with `int` payloads compiles and
runs in a program that has `release`d its `Heap`, so nothing is
allocated and the arms compile to a jump. The cost is three
lines at each call site instead of one comparison, and that is the
point: the third outcome stops being invisible.

`getchar` is not changed by this. It is one byte and its `-1` is
documented; a second spelling of end-of-input in the same program would
be worse than one inconsistent one. What changes is that new API does
not repeat the mistake.

---

## 4. The question §3 did not ask: what the row says

A program that reads `/var/log` today declares
`[fs_read("/var/log")]`, and `lex-sys authority` prints that prefix.
That precision is the thing `authority.md` exists to count.

With a handle, where does the prefix go? Four answers, and none is free:

| | what it costs |
|---|---|
| `read` takes the `Fs(p)` again | The handle is not a capability, just a number. A function given only a handle cannot read from it, which defeats passing one down |
| `read` declares bare `fs_read` | **A regression in the report.** `fs_read` reads as wider than `fs_read("/var/log")`, and a program that switched to handles would look like it gained authority |
| `File[p]` — a refinement on a user type | Preserves everything, and generalises refinement from built-in capabilities to user structs. The largest change of the four |
| The prefix is spent at `open` | The row still carries `fs_read("/var/log")` because `open` performed it. `read` performs a *path-free* label, because the authority was checked once and the handle cannot be widened |

**The fourth.** It is the capability answer — a descriptor is authority
you already hold, and re-checking a prefix you cannot change is
ceremony — and it keeps the report honest at the place the report is
read: the program's own row still names the directory.

The rule it has to satisfy is the one `bulk-io.md` §3.2 pinned when the
same question came up for output: **a program must not look more
powerful for having been written better.** A conformance test belongs
here for the same reason, comparing the authority report of a
`fs_read`-on-a-path program with a handle program over the same
directory, and requiring the prefix to survive.

What `read` performs is then a label that says *this touches a file I
already opened*, which is not `fs_read(p)` and is not nothing.

### 4.1 The label is `file_read`, and the vocabulary already chose it

§6 said this wanted one slice's argument rather than a guess. The
argument is short, because the rule was already there.

**Every label in this language is named after the capability that
discharges it, not after the operation.** `io_read` and `io_write` are
discharged by owning an `Io`; `fs_read(p)` and `fs_write(p)` by owning
an `Fs(p)`; `heap` by owning a `Heap`; `args` by owning `Args`. The
name is the capability, lowercased, plus a direction where the
capability has two.

A `File` is a capability. It is not one `split` hands out — it is
*manufactured* by `open_read`, out of an `Fs(p)` and a path, which is
why `open_read`'s own row carries `fs_read(p)` and pays for the whole
thing once. Afterwards it behaves like every other capability: owning it
discharges what it permits, borrowing it declares it, and it is consumed
exactly once — by `close` rather than by `release`, because ending a
descriptor is a syscall and ending a capability is not.

So the label is the capability's name plus the direction: **`file_read`**.

It carries **no argument**, and that is the same rule rather than a
concession. `heap` and `args` have none because "a heap has no parts to
name and so nothing to narrow". A file *does* have a name — and the
name was spent at `open`. Re-attaching it to `read` is §4's first option
wearing a label instead of a parameter, and it would mean a handle you
could not pass to a function that had not been told where it came from.

Two consequences worth stating, because both are the existing rules
rather than new ones:

* **Owning a `File` discharges `file_read`.** `read(f, into)` borrows,
  so a function that borrows declares `[file_read]` and a function that
  owns the handle outright declares `[]` — exactly what `authority.md`
  §2 established for `main` and the console.
* **`open_read` still names the directory.** Its row is
  `[fs_read("/var/log")]`, so the authority report of a handle program
  and a path program over the same directory agree, which is
  `bulk-io.md` §3.2's rule and the conformance test §4 asks for.

---

### 4.2 What three names cost, which nothing here had priced

Adding a prelude type reserves its name in **every** program, and this
milestone wanted the three most ordinary names in the language.

| name | what it collided with |
|---|---|
| `File` | **31 fixtures** declared `res struct File` as a stand-in resource |
| `read` | `examples/serve/` declares `extern fn read` — libc's, on a socket |
| `close` | the same 31 fixtures had a `close` of their own |

The type keeps its name: `File` is what the thing is, the fixtures were
using it as a *pretend* one because a real one did not exist, and they
now say `Ticket`, which is what the rest of the suite already calls a
stand-in. That churn is the honest price and it is paid once.

The two verbs do not. `read` and `close` are renamed **`file_read` and
`file_close`**, which is `fs_read`/`fs_write`'s shape — the subject,
then the verb — and shares the builtin's name with the label it
performs, exactly as `fs_read` already does. The deciding case is
`examples/serve/`: a program that reads a socket through `Ffi("libc")`
*and* a file through a handle is an ordinary program, and the language
taking `read` would have made it unwriteable.

The rule worth carrying forward: **a builtin's name is reserved against
`extern fn` too**, so a one-word builtin costs every program that wanted
to bind that libc symbol. The prelude has ten of those already; it did
not need three more.

---

## 5. What this does not do

Not seek, not append, not truncate, not directories, not metadata. A
handle that can be read to the end and closed is what `examples/sort/`
is waiting on, and every other verb can arrive when a program asks for
it — which is how `vec.set`, `vec.swap` and the bit operators arrived.

Not a change to `fs_read`. Whole-file reading is the right call for a
file whose size you know and `manifests/`-shaped work keeps using it.
This is the API for a file whose size you do not know, which is the case
that currently cannot be written correctly at all.

---

## 6. Open

| Question | Why it waits |
|---|---|
| ~~What `read`'s effect label is called~~ | **Settled — §4.1: `file_read`.** Every label here is named after the capability that discharges it rather than after the operation, and a `File` is a capability; it carries no argument for the same reason `heap` does not, since the path was spent at `open` |
| ~~`open_read` answers `Result[File, int]`~~ | **Corrected — §2.1.** It cannot: `Result` is `std.result`, a builtin's signature is prelude, and `standard-library.md` says `std` is opt-in rather than a prelude. It answers a prelude `Opened` instead, and `std.fs` may wrap that |
| Writing through a handle | Symmetric, and deliberately not designed with the read side. `bulk-io.md` §3.3 declined to design the input half alongside the output half for the same reason, and that turned out right |
| ~~Whether `End` can be observed twice~~ | **Settled: `End` again**, and it is a fixture rather than a paragraph. POSIX answers 0 at every read past the end, and `Failed(int)` carries an errno — there is no errno for *you already knew*, and inventing one would be a sentinel with a constructor around it, which is the thing §3 exists to stop |
| ~~`examples/sort/`'s 8 MiB ceiling~~ | **Done.** §1.1 — fifteen attempts reach 1 GiB, and a fixture past the old bound is in the conformance suite. It never needed handles, which is worth noticing: the bug the design doc was written to motivate turned out to be separable from the design |
| ~~Telling a person *that* a file failed~~ | **Done**, and not by this document — `standard-error.md`. §1.2's half-fix is a whole one: exit 3 against exit 2 for a script, and a named path on standard error for a person. The blocker was cited here as "`reach.md` §6's standard-error gap", which was a gap no document had recorded |
| ~~Telling a person *why* it failed~~ | **Done.** `Failed(int)` carries the real `errno`, read through `__errno_location()` on glibc and `__error()` on macOS — the same per-platform pair `stderr`/`__stderrp` already needed. A missing file now answers `Failed(2)`, which is `ENOENT`, where `fs_read` answered `-1` and nothing else. This was the one argument for handles that survived §1's other two evaporating, and it is the one that paid |
| Turning an `errno` into a sentence | What `Failed(2)` still is not: a program can print the number and not *No such file or directory*. `strerror` is one `Ffi("libc")` call away and that is the wrong shape — it would make a diagnostic cost the authority to call anything. A table in `std` is the likely answer and no program has asked yet |
