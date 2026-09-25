# Agent-shaped tools

> **Status: measured, then built.**
>
> `docs/reach.md` §1.1 says what decides whether a program is writable:
> not a feature list, whether the authority it needs has a name.
> `docs/agent-errors.md` §2.1 says what a machine reader needs from a
> refusal that a person does not: the fields, not the sentence. Both are
> claims about *this compiler*. This document asks whether they are also
> claims worth building a *runtime tool* on — one an agent calls the way
> it calls this repository's own `Grep`/`Read`, not one a person types at
> a shell — and `examples/seek/` is the answer, built rather than argued.

---

## 1. What "better fits an agent" has to mean, or it means nothing

A rewrite in a safer language is not, by itself, a different tool. GNU
`grep` is not unsafe in the way that matters here — it does not have a
buffer overflow waiting in it. So "no UB" alone is not the pitch; it is
the floor every port in `examples/` already stands on (`base64`, `cut`,
`sort`). What would make a *search* tool specifically better suited to
an agent's own tool-use loop, rather than merely as safe as the one it
already has?

Three properties, each one this repository had already built for a
different reason and none of them assembled into one program before now:

* **An authority report the caller can read before running it.**
  `lex-sys authority --output json` is not new (`docs/under-a-grant.md`),
  but nothing in `examples/` had used it to make a *specific, narrow*
  claim the way `seek` does — see §2.
* **No silent truncation.** `docs/line-reading.md` found `cut` answering
  a *wrong* result on a long line rather than a merely incomplete one,
  and fixed it by growing on the heap instead of capping. An agent
  pointing a tool at a file it has not read yet cannot know if a line is
  long, so a tool that gets quietly wrong past some size is worse than
  one that is slow past it.
* **No UB on adversarial input.** An agent frequently runs a tool over
  bytes it fetched from somewhere it does not control. `defined-
  behaviour.md` §2.1's guarantee already covers this for free; §4 below
  is where it is exercised rather than only claimed.

## 2. `seek`, and the report it makes

```
$ lex-sys authority examples/seek/seek.ls --std --output json
```

names exactly seven labels — `args`, `err_write`, `file_read`,
`fs_read("")`, `heap`, `io_read`, `io_write` — and `bounded: true`
(`docs/under-a-grant.md` §5.1's own flag: no foreign symbol, nothing an
outside reader has to trust the program's own word about). `io_read` is
there because the standard-input fallback is unconditionally reachable
from `main`, whether or not a given invocation takes it; `err_write` is
the usage and per-file error messages. `agent_tools.rs`'s
`seek_reports_a_bounded_authority` checks this stays exactly seven, the
same way `the_report_fails_closed` already checks `cut`'s `bounded` flag.

That is the whole claim: not that `seek` is sandboxed — `fs_read("")` is
the unnarrowed root, same as `cut`/`sort` today, §3.1 is why — but that
what it can reach is *legible*, in seven words, to whatever decided to
run it, before it runs. A person auditing a shell script has to read it;
a supervisor deciding whether to hand a tool to an agent can read the
seven words instead — the same trade `reach.md` §1.1 already named,
applied to one more caller.

## 3. Two honest limits, found while building it

### 3.1 `narrow` takes a literal, so no flag can narrow this tool's `Fs`

`linearity-and-effects.md` §7.4: *"the argument to a narrowing function
must be a literal."* `docs/net.md`'s `bind`/`connect` narrow to a bound
baked into the program at compile time, and that is fine for those,
because a program built for one grant is exactly what `lex-os` deploys.
A general-purpose search tool is the opposite shape — the whole point is
running it against a directory picked at invocation time — and M2 has no
way to narrow `Fs` to a runtime string. `seek` could not have shipped
with a tighter `Fs` than `cut`/`sort` already carry without abandoning
the ability to be pointed anywhere, so it does not try, and says so here
rather than in a comment nobody reads.

What this means for `lex-os`, concretely: a supervisor that wants an
agent to have `seek` scoped to one directory does not get there with a
flag. It gets there by building a copy of `seek.ls` with that directory
substituted for `Fs("")`'s literal, the same way `crates/lex-sys/tests/
conformance/backends.rs`'s own `bind` test generates a program with a
free port substituted in before compiling it (`docs/llvm-backend.md`
§7.21). A capability fixed at build time and proved by the compiler is a
stronger claim than one checked at run time against a flag a caller
supplied — closer to `docs/net.md` §2's inbound bound than to a shell
tool's `--root` — but it means "one binary, any directory" is not this
tool's shape, and pretending otherwise would be the report lying by
omission.

### 3.2 `-m` counts across every file, not per file

GNU `grep -m N` stops after `N` matches **in the current file** and
moves to the next. `seek -m N` stops after `N` matches **total**. Not an
oversight: an agent capping output is bounding its own context, and "at
most `N` lines back, from however many files" is what that means in
practice far more often than "at most `N` from each." Implementing GNU's
own per-file reading would cost a second counter reset at every file
boundary for a distinction the caller this tool is built for rarely
wants — so this diverges and documents it, the same way `cut.ls` already
documents refusing an operand GNU would open (`docs/porting.md`).

## 4. What is not here

No regex: `std.regex` does not exist (confirmed by its absence, not
assumed), so `seek` is a literal substring match, `std.bytes.find`
underneath, the same primitive `cut.ls` and `sort.ls` already use. No
recursive directory walk: nothing in this language lists a directory's
contents, so `seek` searches the files named on its command line, the
same restriction `cut`/`sort` already accept for the files they read.
Neither is a missing feature this document is asking for — `std.regex`
and a `readdir` builtin are both real, sizeable slices with no asker
behind them yet, `standard-library.md`'s own bar.

## 5. Checked

`crates/lex-sys/tests/conformance/agent_tools.rs`: a match, a miss, `-n`,
`-c`, `-m` across two files, a missing file (exit `2`), no pattern given
(exit `2`, usage), reading standard input when no file is named, and a
file containing a NUL byte and non-UTF-8 bytes found and printed without
crashing — `defined-behaviour.md` §2.1's guarantee, exercised on a
program built for exactly the case where an agent cannot vouch for its
input. `agent_tools.rs`'s `seek_reports_a_bounded_authority` checks §2's
report claim directly.
