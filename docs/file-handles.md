# File handles

> **Status: designed, not built.**
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
open_read[&c, &a](fs: &c Fs(p), path: &a [byte]) -> [fs_read(p)] Result[File, int]
close(file: File) -> [] int
```

(`Fs(p)` is `filesystem.md` §3's own schema notation for "whatever
prefix the capability carries", not literal source — real code spells a
concrete one, as `sort.ls` does with `Fs("")`.)

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
read[&f, &b](file: &!f File, into: &!b [byte]) -> [?] Read
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

The row is `[?]` on purpose: §4 is what goes there, and §6 keeps the
name open.

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
already opened*, which is not `fs_read(p)` and is not nothing. Naming it
is open — §6.

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
| What `read`'s effect label is called | §4 settles that the prefix is spent at `open` and that `read` performs *something*. `io_read` is taken, `fs_read(p)` is wrong without a `p`. It wants a name, and a name is worth one slice's argument rather than a guess here |
| Writing through a handle | Symmetric, and deliberately not designed with the read side. `bulk-io.md` §3.3 declined to design the input half alongside the output half for the same reason, and that turned out right |
| Whether `End` can be observed twice | Reading past the end: `End` again, or `Failed`? POSIX says a repeat read at EOF answers 0 again. Probably `End`, and it should be a fixture rather than a paragraph |
| ~~`examples/sort/`'s 8 MiB ceiling~~ | **Done.** §1.1 — fifteen attempts reach 1 GiB, and a fixture past the old bound is in the conformance suite. It never needed handles, which is worth noticing: the bug the design doc was written to motivate turned out to be separable from the design |
| ~~Telling a person *that* a file failed~~ | **Done**, and not by this document — `standard-error.md`. §1.2's half-fix is a whole one: exit 3 against exit 2 for a script, and a named path on standard error for a person. The blocker was cited here as "`reach.md` §6's standard-error gap", which was a gap no document had recorded |
| Telling a person *why* it failed | The half that is left, and the half this document owns: `fs_read` answers `-1` with no reason attached, so `sort` can name the path and not the cause where GNU names both. §3's `Failed(int)` is the designed shape — which makes it an argument for handles that survived §1's other two evaporating |
