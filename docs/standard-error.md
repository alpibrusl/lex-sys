# Standard error

> **Status: measured, then designed.**
>
> Two documents in this repository say the gap below is *"named in
> `reach.md` §6"*. It is not. `reach.md` never mentions standard error
> at all — not in §6, not anywhere — so the citation pointed at a row
> that was never written, twice, and the second citation was copied from
> the first.
>
> The gap is real. §1 measures it, and one of the three things measuring
> it found is a sentence from the previous slice that is false for
> exactly the reason this document exists.

---

## 1. What the absence costs, measured

Every program here fails silently. Not as a figure of speech — the
failure paths produce **zero bytes** on every stream:

| | exit | stdout | stderr | GNU's stderr |
|---|---|---|---|---|
| `sort /nope/missing.txt` | 2 | 0 | **0** | 64 bytes |
| `cut -d, -fx` (malformed list) | 2 | 0 | **0** | 68 bytes |
| `base64 -d` on a bad byte | 1 | 0 | **0** | 22 bytes |

Eleven of the nineteen programs under `examples/` have at least one
non-zero exit; `examples/serve/` has four distinct ones. None of them
says anything, and `sort.ls` carries a comment admitting it:

> *"GNU writes the failing path to standard error and exits 2. There is
> no standard error here yet, so this is the status alone."*

### 1.1 The workaround puts a diagnostic in the data

The only stream a program has is the one its output goes on, so being
loud means being loud *there*. That is not a stylistic compromise; it
corrupts the output. A `cut` that prints `cut: invalid field list` on
stdout, in the pipeline a `cut` belongs in:

```
$ cut -d, -fx < data.csv | sort | wc -l     # GNU
0
$ ours -d, -fx < data.csv | sort | wc -l    # the workaround
1
```

The line `sort` sorted was the diagnostic. Exit status is still 2 and
the consumer still gets a row, which is the failure mode a status code
exists to prevent.

### 1.2 And it loses the message exactly when it matters

Worse, and this is the row that decides the design. `stdout` is fully
buffered when it is not a terminal, and a trap does not flush it. A
program that says what is wrong and then dies says nothing:

```
$ lost > out.txt ; echo $?     # writes "about to fail", then traps
132
$ wc -c < out.txt
0
```

Zero bytes into a file, and zero bytes through a pipe. The diagnostic
is lost in precisely the case — the program died — where it is the only
evidence there is. So the requirement is not merely *a second stream*:
it is a stream that has arrived by the time the next instruction runs.

### 1.3 A silent path is a path nothing compares

`examples/cut/cut.ls` says, above its parser:

> *"Answers `-1` in `from_open` on a malformed list, which `main` turns
> into the exit status GNU uses."*

GNU uses **1**. It used **2**. Measured on the same input, one slice
after the comment was written, by the same hand.

The first version of this section said nothing caught it because the
failure path was untested. **That was wrong, and it is worth correcting
here rather than quietly**, because the true version is sharper.
`cut_agrees_with_gnu_cut` *did* run the malformed spec, and asserted:

```rust
assert_eq!(bad.status.code(), Some(2), "a malformed `-f` list should exit 2");
```

So the path was tested. It was tested **against me** rather than against
GNU — while, eleven lines above, the same test ran eight *valid* specs
against the real `/usr/bin/cut` and compared them byte for byte.

Nothing forced that asymmetry except what there was to compare. On a
valid spec GNU produces output and so do we, so the obvious thing to do
is diff them. On a malformed one GNU produces a *message*, we produced
silence, and the only comparable artefact left was a number — which
promptly got written down from the same belief the comment came from.

**A stream nobody has is a comparison nobody makes.** That is the
argument for this slice with the accident taken out of it, and the fix
is not only the message: `cut_reports_a_bad_field_list_like_gnu_cut`
now asks the reference for its status and its wording, the way the
valid specs always did.

---

## 2. It is not a seventh capability

`standard-input.md` §2 settled this shape when it added reading, and the
argument transfers without change:

> *The capability is what you **hold**. The labels are what you **did
> with it**.*

`Io` is the console. Standard error is part of the console — the same
process, handed the same three descriptors by the same parent, redirected
by the same shell on the same command line. A `StdErr` capability would
be an eighth field on `Split`, a break in every program, and a second
name for authority nobody grants separately.

So: **one capability, a third label.**

### 2.1 What it does widen, said plainly

This is not free and the honest version is worth writing down. Before
this change, an `Io` holder could not reach descriptor 2 from lex-sys at
all — only through `Ffi("libc")`, which `reach.md` §5 establishes is every
authority at once. After it, an `Io` holder can. **A grant of `Io` is
worth more than it was.**

That is acceptable for one reason and it is not "it is only stderr":

**The grant is the capability; the gate is the row.** `authority.md` is
built on reading rows, not on counting capabilities, and a program that
writes diagnostics now says `err_write` in its own signature and in its
report. Anything mediating this — a supervisor, `lex-os`, a reviewer —
gates on the label, which is finer after this change than before it,
because before it the same program said `ffi("libc")` and meant
*everything*.

The alternative fails on its own terms. A separate capability would make
every `main` that wants to be loud thread a second value through to the
failure path, and the failure path is the one place a program is already
unwinding, holding the least, and least able to afford ceremony.

### 2.2 What `Io` discharges

Owning an `Io` outright discharges `io_read`, `io_write` and `err_write`,
the way it already discharges the first two. Borrowing still declares.
Owning a `World` discharges all three, because `split` is reachable from
one.

---

## 3. The operation

```
write_err[&i, &b](io: &!i Io, bytes: &b [byte]) -> [err_write] int
```

The same shape as `write_bytes` (`bulk-io.md` §3), the same borrowed
capability, the same shared slice, the byte count back.

### 3.1 Why the label is `err_write` and not `io_err`

Every other label takes its domain as a prefix: `fs_read`, `fs_write`,
`io_read`, `io_write`. `io_err` would keep that pattern and read as *an
error in io*, which is the one thing it does not mean.

`err_write` names the **stream** first, and the stream is the right unit
because it is the unit a reader can act on: `1>` and `2>` are two
different redirections, and a row that distinguishes them tells someone
holding a shell something they can use. It also leaves `err_read`
un-spelled and unreachable, which is correct — there is no reading from
standard error.

### 3.2 Why there is no per-byte twin

Standard output has two primitives: `putchar` for a byte and
`write_bytes` for a slice. `bulk-io.md` §1 measured why — 12.8×, and a
byte at a time through stdio is the difference between a program that is
usable and one that is not.

None of that applies here. **A diagnostic is short and stderr is never
the hot path**, so the per-byte primitive would be the redundant one
rather than the fast one. `putchar` exists because it came first, not
because a second primitive was wanted; repeating that here would be
repeating the history rather than the design.

A program that needs a *number* on standard error formats it into a
buffer and writes once — `std.buffer` and `std.fmt` already do exactly
that, and no program here has asked. When one does, it will ask for a
function in `std.io`, not for a builtin.

### 3.3 What it reaches

`fwrite(ptr, 1, len, stderr)`.

`bulk-io.md`'s reason for choosing `fwrite` over POSIX `write` does not
carry here and it is worth saying so rather than borrowing it: there
`write_bytes` had to share a stream with `putchar` or the two would
interleave wrongly, and nothing else writes to this stream at all. A
`write(2)` on descriptor 2 would be correct too.

The reason is the guarantee. C says `stderr` is *not fully buffered*,
and that is a promise about the `FILE *` — so taking the `FILE *` is
taking the promise rather than re-deriving it, and it keeps one
mechanism for both streams instead of two.

It is what makes §1.2's measurement come out the other way: a diagnostic
written before a trap has already left. A property to test rather than
assume, so `a_diagnostic_survives_a_trap` asserts both halves — the
message on standard error arrives, and the same message on standard
output does not.

---

## 4. What the report must not say

`lex-sys authority` prints a negative half — *"never touches"* — and
`authority.md` §2.2 is why it is the interesting half: an absent label is
a proof.

A new label with no entry in that table produces a specific lie. A
program that writes only diagnostics would perform `err_write`, hold no
other console label, and be reported as

```
never touches
    the console
```

which is false about a program whose entire output is on the console.
So `err_write` joins `io_read` and `io_write` under **the console**, and
the test that fixes it is `a_diagnostic_only_program_touches_the_console`.

The positive half needs nothing: it prints the labels it finds, and
`err_write` is one.

---

## 5. Ordering, and what is not promised

Standard output is buffered and standard error is not, so a diagnostic
can appear *before* output the program produced earlier. That is C's
behaviour, it is what every coreutil does, and this does not correct it:
flushing stdout inside `write_err` would make a diagnostic depend on the
state of an unrelated stream, and would make the two streams'
interleaving a language promise that no other program in the pipeline
keeps.

**What is promised is per-stream order**, which is the same promise the
output stream already makes for mixing `putchar` and `write_bytes` —
both go through one stdio stream, and `tests/accept/bulk_write.ls`
checks the interleaving. Every byte written to standard error appears in
the order it was written, and `tests/accept/standard_error.ls` checks
that. Across the two streams, nothing, and nothing in either fixture
asserts one.

---

## 6. What this does not do

Not a `Diagnostic` type, not an error-formatting convention, not
`perror` and not `errno`. GNU's message for a missing file names the
errno string; here the path is known and the reason is not, so the
programs in §7 say what they know:

```
sort: cannot read: /nope/missing.txt
```

Reading the reason back out of the operating system is `errno`'s job, and
`filesystem.md`'s `-1` does not carry one. That is a real gap, it is the
same gap `file-handles.md` §3's `Failed(int)` was designed to fill, and
it is that document's to close rather than this one's.

Not a logging facility, not levels, not a `--quiet` convention. One
stream, one operation.

---

## 7. What became loud

| | now says | exit | against GNU |
|---|---|---|---|
| `sort` on a file it cannot read | `sort: cannot read: <path>` | 2 | same status, same wording, **less the errno string** (§6) |
| `sort` on a file too large to hold | `sort: file too large: <path>` | 3 | GNU has no such case — it spills to disk |
| `cut` on a malformed field list | `cut: invalid field value '<spec>'` | **1**, per §1.3 | byte for byte |
| `cut` on an unrecognised argument | `cut: usage: cut -d<c> -f<list>` | 1 | same status; GNU names the option and this does not |
| `base64 -d` on a byte outside the alphabet | `base64: invalid input` | 1 | byte for byte |

Five diagnostics, three programs, eleven calls to `error_all`.

Four of the five are in the conformance suite, run against the system's
own GNU tool where there is one. The fifth is not, and the reason is
worth stating rather than hiding: reaching `sort`'s ceiling needs a file
past 1 GiB, and the run takes **9.3 s and 1,025 MB of resident memory**
— measured, on a 1.1 GB sparse file, which is where the wording in the
table comes from. That is a fixture two CI runners would feel, so it
stays a hand check, and `read_file`'s *other* failure — the one that
shares its exit status with nothing — is the one the suite runs.

---

## 8. Open

| Question | Why it waits |
|---|---|
| Why a file could not be read | §6. `fs_read` answers `-1` and the reason is gone. `file-handles.md` §3's `Failed(int)` is the designed shape and this document does not pre-empt it |
| A number on standard error | §3.2. Writable today through `std.buffer`; it wants a `std.io` function, and no program has asked |
| Whether `write_err` should flush stdout | §5 says no, and says why. It is the sort of decision that looks obvious in the other direction the first time two streams interleave confusingly in a terminal, so it is recorded rather than closed |
| A diagnostic convention, and where the name in it comes from | The program's own name is written out at all eleven call sites in §7, and the shape `<name>: <what>: <detail>` is the same in four of the five — a library function forming, on the same evidence the last five arrived on. What stops it being obvious is the name. GNU takes its prefix from `argv[0]`: invoked as `/usr/bin/cut` it calls itself `/usr/bin/cut`, which `cut_reports_a_bad_field_list_like_gnu_cut` had to compare around. A literal is the other answer and it is not plainly worse. `arg(g, 0)` needs the `Args` capability at the failure site, which `base64` has already released by then — so the convention and the capability are one question, not two |
