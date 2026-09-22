# The first page

> **Status: settled and built.**
>
> [`agent-errors.md`](agent-errors.md) §1 measured what this project's
> *machine* audience had — 125 message shapes, 101 of them seen once,
> one error per invocation — and [`AGENTS.md`](../AGENTS.md) was the
> answer. This is the third audience: a person deciding whether the
> project is worth their attention. It had never been measured either,
> and this time the instrument arrived on its own.

---

## 1. A reader wrote fifteen proposals, and eight of them were shipped

Someone outside the project read `README.md` and wrote a design
proposal: fifteen numbered items, a phased roadmap, a comparison table,
a success list. It is a careful document and its thesis is right — *"a
native language for programs that must prove what they are allowed to
do"* is a better one-line statement of this project than the README
had.

Scored against what exists:

| | Proposals |
|---|---:|
| **Already built**, with a design document behind each | **8** |
| Partly built | 4 |
| Not started | 2 |
| Misframed — assumes a feature this language does not have | 1 |

The eight are capabilities as values, canonical effect rows, the
unification of the two, lexical borrowing instead of a borrow checker,
capability-mediated FFI, a native sandbox, program identity, and agent
execution. Each has a document, a suite and a roadmap row.

The proposal's phases 1 through 4 are done. It opens at phase 1.

### 1.1 Which is not the reader's mistake

The document is evidence about the README, not about its author. A
proposal that re-proposes eight shipped features is a measurement of
what the first page failed to say, and it is a better one than any
number this project could have invented, because nobody wrote it to
be one.

---

## 2. Three words the README never uses

Counted over 572 lines:

| Word | Occurrences |
|---|---:|
| `sandbox` | **0** |
| `audit`, `evidence` | **0** |
| `agent`, in the execution sense | **0** — the two hits are about `AGENTS.md`, the *writing* guide |
| `lex-os` | **1**, inside a parenthesis about narrowing |
| program identity, `ids` | 1 |

Three of the proposal's fifteen items — the sandbox, execution evidence
and agent execution — live in `lex-os`, and **the words for them do not
appear on this project's first page at all**. A reader could not have
known. All three were re-proposed.

That is the whole finding, and it is not about tone or length. The
README describes a language and the project is a language *and its
runtime*, and only one of those was on the page.

### 2.1 And the shape said something else again

| Section | Lines | Share |
|---|---:|---:|
| Performance expectation | 180 | **31%** |
| The language in one page | 132 | 23% |
| What exists | 70 | 12% |
| Try it | 67 | 12% |
| Everything else | 123 | 22% |

The largest section by far was about the thing this project is weakest
at, and *"Not a usable language yet"* was in the third paragraph. What
exists was one 70-line block of prose naming some thirty features in
sentences.

None of those lines is wrong. `overflow-cost.md`, `aliasing.md` and
`gpu.md` are all measured and all honest, and the honesty is the point
of them. But a first page is read in the order it is written, and this
one spent a third of itself on the gap to C before it had said what the
project is for.

---

## 3. What the rewrite is for

One job: **a reader who stops after the first screen should come away
with the thesis and the fact that it is built and enforced twice.**

Four changes, and no new claims:

1. **A map of the ecosystem, which did not exist.** §4.
2. **What exists as a table**, one row per capability, each linking the
   document that settled it — so the answer to "is X built?" is a
   lookup rather than a reading.
3. **The performance section compressed** to its numbers and its one
   structural cause, with the essays staying in the documents that
   measured them. `aliasing.md` §6, `gpu.md` §2.3 and
   `benchmarks-game.md` already carry every argument the 180 lines
   made; what the first page owes a reader is the number and the
   reason, not the derivation.
4. **The status line says what is usable**, rather than only what is
   not. Both halves are true and only one was there.

What it does **not** do: soften anything. The gap to C is 1.17×–2.58×
and stays on the page; "not a usable language" stays on the page; the
three-word count above is why it moved, not why it shrank.

---

## 4. Where lex-sys sits, which nothing said before

Three repositories, one idea, and **two of them are wired together**:

```
                    lex-lang
              the high-level language
        16 crates: syntax, ast, types, store,
           vcs, jit, lsp, bytecode, trace
                        │
                        │  lex-types::trust::Grant
                        │  lex_syntax → lex_ast → lex_types
                        ▼
                     lex-os
            the autonomous-agent runtime
      manifest + grant → static check (lex-os-check)
              → perimeter → supervisor → audit


                     lex-sys
                     this repo
        a second, native language, same worldview
         — and no code edge to either of them —
```

`lex-os` takes its `Grant` from `lex-lang`'s `lex-types` and runs the
agent's programs through the **real Lex front end**, so one declaration
is enforced twice: statically by `lex-os-check` before the program
loads, and at run time by the supervisor.

`lex-sys` shares that worldview and **shares no code**. It mentions
`lex-os` in exactly two comments, and `lex-os` does not depend on it at
all. Saying so is the point: the proposal assumed an integration that
does not exist, and a first page that implied one would have been the
reason.

### 4.1 The two joins, both named and both gated

| Join | State |
|---|---|
| lex-sys code in `lex-vcs` | 81% of that crate is already language-agnostic. Gated on a **plateau** in the effect vocabulary, not on a feature — [`hash-stability.md`](hash-stability.md) |
| lex-sys code under a lex-os grant | Measured in [`under-a-grant.md`](under-a-grant.md): **not** a compiler integration. `authority --output json` is already the right interface and the grant's filesystem dimension works through it; `network` and `exec` do not, because both are libc |

The second is worth writing down as a row rather than a paragraph,
because it is the one place where this project's thesis and its runtime
would meet in code rather than in agreement.

---

## 5. The suite

| Test | Shows |
|---|---|
| `every_documentation_link_resolves` | every local link in `README.md`, `AGENTS.md` and `docs/` points at a file that exists — run by hand at the end of every slice until now, which is the wrong place for it |
| `the_readme_commands_still_work` | the commands the first page shows a reader are commands the compiler still has |
