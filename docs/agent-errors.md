# Refusals a machine can read

> **Status: measured, then designed.**
>
> lex-lang's `docs/AGENT_GUIDELINES.md` is a contract with a *reader
> that is a program*: `lex check --output json` answers a stable
> `rule_tag`, and §4's rule is **repair, don't regenerate**. lex-sys has
> none of that. It has 196 must-reject fixtures, each of which answers
> a sentence written for a person.
>
> §1 measures what that costs a machine. §2 is the part of lex-lang's
> answer that transfers and the part that cannot, and the line between
> them is not where the guidelines page puts it.

---

## 1. What a refusal gives a machine today

`check` run over every fixture in `tests/reject/`:

| | |
|---|---|
| refusals carrying `file:line:col` | **193 / 196** |
| distinct message *shapes*, with names and numbers normalised out | **125** |
| shapes that occur exactly **once** | **101** |
| errors reported per invocation | **1** |
| machine-readable form | **none** |

Three things follow, and they are independent problems.

### 1.1 There is no vocabulary, only prose

A program that wants to know *which rule it broke* has 125 sentence
patterns to match, and 101 of them it will meet once. lex-lang answers
that question with one field: **16** `rule_tag`s cover every type error
it can raise.

The three refusals with no location are program-level — `no main
function`, and the two about `main`'s shape. They have nowhere to point
because the thing that is wrong is the program rather than a span in it,
which is a real answer and still leaves a consumer with a message shape
it has to special-case.

### 1.2 One error per invocation

```
$ cat three.ls
fn a() -> [] int { return true; }
fn b() -> [] int { return oops; }
fn c() -> [] bool { return 1; }
fn main() -> [] int { return 0; }

$ lex-sys check three.ls
three.ls:1:27: error: expected `int`, found `bool`
```

Three independent errors in three independent functions; one reported.
Across two files, the same: the first one found, and nothing about the
second file. A parse error anywhere pre-empts every type error in the
program.

**This costs a person almost nothing and an agent a great deal.** A
person has the file open and sees the other two; an agent pays a
compile, a read and a turn per error. lex-lang returns
`Vec<PositionedError>` — the plural is the point.

### 1.3 The data is structured, then thrown away

The messages already end in the fix:

> *"`verbose` performs `args`, which its row `[]` does not declare;
> narrow the body or widen the row"*

At the point that sentence is built, the checker holds the function, the
label and the row as values. `format!` turns three facts into one
string, and a consumer's only way back is a regular expression over
English.

---

## 2. Which half of lex-lang's answer transfers

**The checker half transfers exactly.** `lex check --output json` emits

```json
{
  "kind": "type_error",
  "rule_tag": "EFFECT_NOT_DECLARED",
  "position": {"file": "src/handler.lex", "line": 14, "col": 22},
  "rule_explanation": "effect `fs_write` reached at this position is not declared …"
}
```

and lex-sys has every ingredient: `Diagnostic { message, span }`, a
`SourceMap` that resolves a span to `file:line:col`, and `--output json`
already shipped on `authority`. What is missing is the tag and the
plural.

**The repair half does not transfer, and the guidelines page is
misleading about why.** §4.2 shows `suggested_transform` inline in the
checker's JSON, next to `rule_tag`, as though the two were siblings.
They are not. In lex-lang's source, `rule_tag` lives in
`lex-types/src/rules.rs` — the checker — while `suggested_transform`
appears only in `lex-store`, `lex-vcs` and `lex-lsp`: it is a repair
hint recorded against an **op id**, consumed by `lex repair --apply`,
and written back as a `RepairAttempt` attestation that
`lex blame --with-evidence` can follow.

lex-sys has no op log, no store and no attestation graph, and
`ROADMAP.md` keeps it that way — the content-addressed AST is here, the
VCS built on it is lex-lang's. So **`lex-sys repair` is not the next
slice and may never be one.**

### 2.1 But the hint is not the attestation

What an agent needs from §4.2 is not the `RepairAttempt`. It is the
*fields*: which label, which row, which function. Those are §1.3's
thrown-away values, and emitting them costs nothing but the decision to
carry them.

So this document takes the rule-tag half whole, takes the hint's
**data** without its machinery, and leaves the repair *command* alone.

---

## 3. What a tag names

> **A tag names the rule a reader would look up, not the sentence the
> checker wrote.**

Two fixtures that break the same rule share a tag however differently
they are worded; two that break different rules get different tags
however similar the wording. That is the only line that keeps a
catalogue from drifting back into 125 entries.

Measured twice, and the second measurement is the one that matters.

Classifying all 196 **refusals** gives 125 shapes falling into 38 rules.
But a refusal is what a fixture reached, and the catalogue has to cover
what the *checker* can say. Classifying the **187 error sites in the
source** instead, and then correcting the classification site by site
while tagging them, gives **52 rules** — and the largest are the ones a
reader would expect:

| | rule |
|---|---|
| 26 | `type-mismatch` |
| 19 | `linear-value-unconsumed` |
| 12 | `effect-not-declared` |
| 12 | `unknown-name` |
| 10 | `reference-escapes-region` |
| 9 | `mode-bound-violated` |
| 8 | `linear-use-after-move` |

**52 against lex-lang's 16, and that is not bloat.** lex-sys checks four
things lex-lang does not have: linearity, regions, capabilities as
values, and defined-behaviour arithmetic. Those four account for
`linear-*`, `reference-escapes-region`, `borrow-conflict`,
`capability-*`, `region-*`, `mode-bound-violated` and `constant-traps`
on their own.

Two sites are structural rather than rules: one wraps another
diagnostic to add *"(checking `f` instantiated at `T`)"*, and one is the
parser's `err` helper. Both carry the rule of whatever they wrap or are
handed, rather than earning a tag.

### 3.1 A tag is stable

Once shipped, a tag never changes meaning. New rules get new tags; a
rule that splits gets siblings rather than repurposing the parent. This
is lex-lang's rule and it is adopted verbatim, for the reason
`canonical-ast.md` gives about hashes: a stable name is worth
something only if it is actually stable.

### 3.2 Counting the rules made the coverage countable

43 rules had a must-reject fixture. **52 exist. Nine did not.**

| rule with no fixture | |
|---|---|
| `literal-form` | an unterminated string, a `0x` with no digit, a float literal that is not one |
| `foreign-declaration` | a builtin redeclared as foreign |
| `region-mismatch` | a `where` clause naming a region the signature does not take |
| `module-not-imported` | a qualified name with no `import` above it |
| `not-a-function` | a local binding in call position |
| `not-a-place` | assignment to an expression |
| `not-a-tuple` | `let (a, b) = 7` |
| `not-an-enum` | `Point::Origin` where `Point` is a struct |
| `not-public` | reaching a private name from another module |

The conformance harness says, at the top of the file: *"Adding a rule to
the language means adding a fixture here… the discipline starts at M0,
when it is cheap."* It was kept for 43 rules out of 52 — **83%**, where the
sentence reads as 100%.

That gap was not findable before. "Every rule has a fixture" is a claim
about a set nobody had enumerated, so it could not be checked, so it
drifted. **A catalogue is what makes a coverage claim falsifiable**, and
that is an argument for tagging that has nothing to do with agents.

Eight of the nine fixtures are written in this slice.
The ninth, `not-public`, the single-file harness **cannot** reach: `pub`
is about reaching another module, a file declares at most one module,
and a fixture here is one file. It is covered by
`the_module_rules_are_enforced_across_files`, which is where a second
file exists, and `every_rule_has_a_fixture` names that exception rather
than pretending the count is clean.

Every reject fixture now declares the rule it is a fixture *for*:

```
//~ ERROR performs `args`, which its row [] does not declare
//~ RULE effect-not-declared
```

Declared rather than derived, which is the same argument as §3: a
fixture says which rule it exists to pin, and a checker that starts
answering a different one fails the suite instead of quietly retagging.

---

## 4. Reporting every error rather than the first

`check` stops at the first refusal. That is not a rule anyone decided —
it is `Result<_, Diagnostic>` at 162 error sites and `?` between them.

The rule this replaces it with:

> **Independent errors are all reported. Dependent ones are not
> invented.**

Which is decidable here because of something the standard library slice
already established: **checking is total and runs per declaration**
(`standard-library.md` §5.2 — *"every function, once"*). A function
whose body is ill-typed does not make the next function's body
unknowable, so each declaration's refusal is collected and checking
carries on.

Two places where it stops rather than guessing:

* **A parse error ends the file.** A file that did not parse has no
  reliable second error, and a recovering parser inventing one is how a
  tool teaches an agent to chase phantoms. One parse error, and the
  program is refused.
* **A declaration that failed to check does not have its *uses*
  reported.** If `f`'s signature could not be made sense of, every call
  to `f` is noise.

Errors come out in source order, which is the order checking already
runs in.

---

## 5. The shape on the wire

```sh
$ lex-sys check tests/reject/args_effect_undeclared.ls --output json
```

```json
{
  "refused": [
    {
      "rule": "effect-not-declared",
      "message": "`verbose` performs `args`, which its row [] does not declare; narrow the body or widen the row",
      "explanation": "Every effect a function's body performs appears in its row, and a function performs what the capability it was lent authorises.",
      "position": { "file": "tests/reject/args_effect_undeclared.ls", "line": 11, "column": 1 }
    }
  ]
}
```

An **envelope with a list**, not a stream of bare objects, because §4
makes the plural the normal case and a consumer should not have to
discover how many it got by counting lines. It matches
`authority --output json`, which is one object rather than a bare array
for the same reason.

* `rule` — §3. The stable name.
* `message` — the sentence a person reads, unchanged. The prose is not
  the problem; being *only* prose was.
* `explanation` — what the rule enforces, independent of this
  occurrence. lex-lang's `rule_explanation`, and one per tag rather than
  one per error.
* `position` — absent for §1.1's three program-level refusals, rather
  than invented.

The exit code is unchanged: **1** for a refused program. Semantic exit
codes already exist here (1 refused, 2 usage, 3 environment) and a
machine-readable body does not change what happened.

### 5.1 `fix` was designed here and is not shipped

An earlier draft of this section carried a fifth field:

```json
"fix": {"kind": "widen-row", "function": "verbose", "label": "args"}
```

§2.1's argument for it still stands — the data is in the checker's hands
at §1.3's `format!` and is thrown away, and emitting it needs none of
lex-lang's store. **It is cut anyway**, because the argument *for* it is
about a consumer nobody has watched. `rule` plus `message` plus
`position` is already everything an agent needs to locate and classify;
whether it then needs the fix spelled out as data, rather than read from
the sentence that already spells it out in English, is a question a
transcript answers and a design document guesses at.

So it is §7's, with the evidence that would settle it named: an agent
that has `rule` and still repairs the wrong thing. Shipping a field on
the chance it helps is how an interface acquires parts nobody reads —
and a field, once shipped, is as hard to withdraw as a tag.

---

## 6. What this does not do

Not an LSP. Not `repair --apply`, for §2's reason. Not warnings — there
are none in this language and this does not introduce a severity field
to hold the absence. Not a change to a single existing message: §5 adds
fields beside the prose rather than rewriting it, so the human-facing
output of every one of the 196 fixtures stays byte for byte what it was.

That last is a property worth testing rather than intending, and
`the_prose_is_unchanged_by_the_json` is where it is tested.

---

## 7. Open

| Question | Why it waits |
|---|---|
| A `fix` field, and a `lex-sys fix` verb | §5.1. The field was designed and cut: what would settle it is a transcript where an agent has the rule and still repairs the wrong thing. The verb is further out — applying a transform to source text is a different job from checking it, and there is no store here to record the attempt in |
| `AGENTS.md` for lex-sys | lex-lang's contract says a downstream repo copies `AGENT_GUIDELINES.md` as `AGENTS.md`. lex-sys is not a Lex codebase — different language, different rules — so it needs its own, and it has **none**: an agent writing lex-sys today has 41 design documents and no entry point. Larger than this slice and the obvious one after it |
| `lex-sys skill`, the CLI surface as data | lex-lang has `lex skill`; lex-os emits acli envelopes. This adds the third piece — machine-readable *errors* — and the surface is the piece still missing |
| Warnings | There are none. If one is ever added, §5's object needs a severity and every consumer needs to handle it; adding the field before the first warning would be designing for a language that does not exist |
| Whether `build` and `run` answer JSON too | §5 is about `check`. `build` fails for reasons that are not the program's — a linker, a missing `cc` — which is exit 3's territory and a different vocabulary |
