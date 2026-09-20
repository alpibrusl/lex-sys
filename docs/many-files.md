# A program in more than one file

> **Status: settled, and built in the same change.** The gate for the
> sentence three design docs have now had to write: *"there is nowhere to
> put a library."* `heap.md` §5 says it of `Rc` and `Gen`,
> `arguments.md` §5 says it of flag parsing, and every future stdlib
> function says it too.
>
> It is also where `canonical-ast.md` §1 stops being aspirational. That
> section has claimed since M0 that "moving a function between files
> changes nothing about it". With one file there were no files to move
> between.

---

## 1. What is missing

Everything this language can express, it can express in one file — and one
file is where all of it has to go. There is no `std`, because a stdlib is a
set of declarations somebody else wrote and there is no way to bring them
in. There is no `Rc`, no `Vec`, no flag parser, for the same reason: §9 of
the M2 document calls those **libraries, not language features**, which is
only a meaningful distinction once a library can exist.

The epic's non-goals exclude "non-flat module layout" from minimal. This
document takes that literally: a program gains more than one file, and
gains nothing else.

---

## 2. A program is a set of files

```sh
lex-sys build main.ls util.ls list.ls -o app
lex-sys check main.ls util.ls
```

Named on the command line, in any order, and that is the whole of it. No
`import`, no search path, no file-name-to-module-name rule.

### 2.1 Why not `import`

`import "util.ls"` would make a program self-describing, which is a real
advantage, and it needs three things this document would rather not decide
yet: how a path resolves (relative to what?), what happens when two files
import each other, and where a *library* lives when it is not next to the
program. Each has a defensible answer and none is obvious, so they get
their own document when somebody needs them.

A list of files is what C has, and it is enough to put a library somewhere
other than the program. That is the whole ambition here.

### 2.2 The namespace is flat and shared

Every top-level declaration is visible in every file. There is no `pub`,
no privacy, and no qualified name — `push` is `push` wherever it was
written.

So a name declared twice is an error, whether the two declarations are in
one file or in two. That is not a new rule: it is `duplicate_function.ls`,
which has been enforced since M1, now noticing a second file.

The cost is real and worth stating: with no privacy, a library's internal
helper is as visible as its interface, and two libraries that both want the
name `len` cannot be used together. Both are what namespaces and visibility
are for, and both arrive with them.

---

## 3. Identity is content, not location

`canonical-ast.md` §1: *"A unit is one top-level declaration… Moving a
function between files, or reordering declarations, changes nothing about
it."*

That has been the design since M0 and until now it could not be checked,
because there was one file. It can be now, and it is:

> The same function in two different files has the same `BodyId` and the
> same `SigId`. Moving a declaration from one file to another changes
> nothing about its hashes.

Nothing had to be added for this to hold — a unit hashes its own content
and has never seen a file name. What is new is a test that would fail if
that ever stopped being true.

---

## 4. Spans became global

The one piece of real surgery, and it is worth a section because the
obvious design is the wrong one.

A `Diagnostic` carries a message and a `Span`, and a `Span` is a pair of
byte offsets. With several files, an offset alone no longer says where the
error is — so the obvious move is to put a file id in `Diagnostic`.

That would mean touching every place that constructs one, of which there
are hundreds, to say something the offset already determines. So instead:

> **A `Span` is an offset into the whole program's source**, not into one
> file's. Each file is given a base offset when it is parsed, and a
> `SourceMap` turns an offset back into a file, a line and a column when a
> diagnostic is rendered.

No `Diagnostic` call site changed. The checker never learns that files
exist, which is correct — it is checking a program, and a program is a set
of declarations however they were spelled across how many files.

---

## 5. What one file still means

* **`lex-sys print <file>`** renders exactly one file, and round-trips it
  exactly as before. Printing is per file because printing is about text,
  and text is what a file is.
* **`lex-sys ids <file>`** prints that file's declarations' hashes. Per
  file for the same reason, and §3 is the property that makes the choice
  harmless.
* **`main`** must be declared exactly once across all the files, which is
  the rule it already had within one.

---

## 6. What this does not add

* **No namespaces and no visibility.** §2.2 says what that costs.
* **No `import`.** §2.1 says what it would need.
* **No separate compilation.** Every file is parsed and checked on every
  build. Incremental compilation is explicitly outside "minimal" (#1), and
  content-addressed units are the right foundation for it when it comes.
* **No circular dependency detection**, because there are no dependencies
  to be circular: the files are one flat program, and a function in the
  first may call one in the last.

---

## 7. Open

| Question | Why it waits |
|---|---|
| `import` | §2.1: path resolution, cycles, and where a library lives |
| Namespaces and visibility | The other half of §2.2's cost |
| Separate compilation | Wants `import` first, so a unit knows what it depends on |
| A standard library | Wants somewhere to *ship* it from, which wants all of the above |

---

## 8. The must-reject suite

Two of these are old rules noticing a second file, which is the point:
nothing new had to be invented.

| Case | Rule | § |
|---|---|---|
| The same function declared in two files | A name is declared once per program | 2.2 |
| `main` in two files | Exactly one entry point | 5 |
| No `main` in any file | Same | 5 |

And the accepting counterpart, plus the property test §3 exists for:

| Case | Shows |
|---|---|
| A program split across three files | A call in the first file to a function in the last |
| The same declaration in two different files | Identical `SigId` and `BodyId` |
