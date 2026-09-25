# How often does a hash actually move? Two rates, and they disagree

> **Status: measured, and the instrument turns out never to have been
> tested.**
>
> `canonical-ast.md` §8 keeps three things off the contract list and says
> why the list might never empty:
>
> > *"The point is the **rate**: the question of whether this section can
> > ever be emptied is a question about how often these actually move,
> > and nothing was measuring that."*
>
> `ROADMAP.md`'s `lex-vcs` row is gated on the same number — *"the design
> doc is worth writing when that number exists"*. This is the number, and
> there are two of them.
>
> **The encoder has moved 20 times and the golden fixtures have observed
> none of them**, because they landed after the movement stopped.
> **The language has moved enough that 71% of this repository's own `.ls`
> history no longer type-checks** — and the single largest cause is one
> effect label being split in two.
>
> A content-addressed VCS keyed on these hashes inherits the second rate,
> not the first.

---

## 1. The encoder rate: zero, out of zero observations

`crates/lex-sys-id/tests/golden.rs` pins 35 fixtures, one per node
family. §8 introduced them for exactly this purpose: *"It is not a freeze
and a failure is not a bug report — it asks which of two things
happened."*

| | |
|---|---:|
| commits touching `crates/lex-sys-id/src/` | **20** |
| …of those, since the goldens landed | **0** |
| commits since the goldens landed | 14 |
| golden hashes that moved in them | **0** |

So the fixtures have been green for fourteen commits, and that is worth
less than it looks: **none of those commits changed the encoder.** The
instrument was installed after the thing it measures stopped happening.

One commit since did touch `crates/lex-sys-syntax/src/ast.rs` — file
handles (#65), which added three prelude type names. It added no *node
kind*, so no tag moved, and the goldens were right to stay still. That
is one observation of the right kind and it passed.

**The honest reading: the encoder rate is unmeasured, not low.** It will
become measurable the first time a milestone adds a node, and §8's
sentence about "every milestone from M2 on adds nodes" is the reason to
expect that to happen rather than not.

> **A second observation, same kind (#70).**
> [`character-literals.md`](character-literals.md) adds a *spelling* —
> `'a'` is the integer 97 — and rewrote 119 sites across `std/`,
> `examples/` and `tests/accept/` onto it. No golden hash moved, and
> `a_character_literal_hashes_as_its_integer` checks the two spellings
> against each other. Two observations, two passes, and the encoder rate
> is still unmeasured: a change designed to move no tag cannot show that
> a moved tag would be caught.

---

## 2. The language rate: 71% of its own past is unreadable

The other rate needs no new instrument, because git has it. Every
distinct revision of every `.ls` file under `std/` and `examples/` across
all **67** commits, compiled by **today's** binary — one compiler over
every revision, so anything that fails is the language having moved and
never the encoder:

| | |
|---|---:|
| distinct file revisions | **117** |
| today's compiler still reads | **34** (29%) |
| no longer parses or checks | **83** (71%) |

And the causes are not spread out. Classified by first error:

| revisions | why |
|---:|---|
| **35** | `performs io_write, which its row [io] does not declare` |
| 8 | no effect row at all — written before rows existed |
| 8 | a type or function declared twice (files later split apart) |
| 5 | needs a sibling file, or a name that moved |
| 4 | an unknown type |
| 3 | a builtin's arity changed |

> **Re-measured (#85).** [`editions.md`](editions.md) §2 replays the
> same history with a harness that is in the repository
> (`scripts/history.py`) and gives each file the program it belonged to:
> **58 of 141** revisions (41%) no longer check, and the `io` split is
> still the largest class, at 45. §2 there also shows that an alias
> cannot absorb it, because rows are exact.

**One label rename accounts for 42% of the unreadable past.** When
`standard-input.md` §2 split `io` into `io_read` and `io_write` —
correctly, and for reasons that document argues well — it invalidated
thirty-five file revisions' worth of identity in a single commit.

Nothing about that is a bug. It is what a language doing its growing in
public looks like, and every one of those changes was the right call at
the time. The point is only that it is the rate that a hash-keyed tool
would actually experience.

---

## 3. What this means for `lex-vcs`

`ROADMAP.md`'s row says lex-lang's `crates/lex-vcs` is *"81%
language-agnostic already"* and that the gate is *"this side: §8"*. The
two rates say which side of §8 matters.

* **The tag values** (§8's first bullet) are stable in practice and
  unmeasured in principle. Freezing them is a table nobody has had to
  write yet.
* **What moves is the vocabulary a body is written in.** A body's hash is
  a function of its AST, and its AST mentions effect labels, builtin
  names and arities. Those changed 46 times in 67 commits.

So an op DAG built on these hashes would not mostly record edits a
programmer made. It would record the language changing underneath
programs nobody touched — thirty-five of them, at once, for one rename.

That is not an argument against the design. It is an argument about
**when**: the row's instinct to wait was right, and the thing to wait for
is not a number but a *plateau* — a stretch of commits in which the
effect vocabulary and the builtin surface do not move. At the time this
was written, this repository had not had one yet: the last four slices
before it added `err_write`, `file_read`, three prelude types and three
builtins.

> **Corrected (`ROADMAP.md`'s `lex-vcs` row, current as of #119).** That
> was true when written and is not any more. `builtin.rs` and `defs.rs`
> — every builtin, every effect label, every prelude type's own row —
> last changed at #92, the third `Net` slice. The 27 commits since
> (#93 through #119, an entire second backend designed and built start
> to finish) touch none of `crates/lex-sys-ir/`, `crates/lex-sys-types/`
> or `crates/lex-sys-syntax/` at all — `git diff 52daddb..2999a7d --
> crates/lex-sys-ir/ crates/lex-sys-types/ crates/lex-sys-syntax/` is
> empty. The plateau this section said had not happened has now
> happened, measured the same way §1 and §2 above were: by reading the
> commit range rather than assuming it. Whether 27 commits is *long*
> enough — long enough that a `lex-vcs` design written against it would
> not immediately need revising — is a judgement call this section
> leaves open rather than answers for it.

---

## 4. What this does not say

* **Not that the hashes are unstable.** Within one build they are exact,
  and 61 relational tests plus 35 golden fixtures say so. The instability
  measured here is *across language versions*, which §8's third bullet
  already refuses to make a claim about — this puts a number on the
  refusal.
* **Not that 71% is a defect rate.** Every one of those revisions
  compiled when it was written. The figure measures how far the language
  has travelled, and a young language travelling is the intended
  behaviour.
* **Not a measurement of the encoder.** §1 is explicit that the encoder
  rate has zero observations. Anyone reading §1 as "the encoder is
  stable" has read it backwards.
