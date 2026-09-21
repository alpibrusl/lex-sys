# The authority surface

> **Status: settled and built, and §12's question is answered `no`.**
>
> *"Capability release at `main`. Four `release` calls in §8.3 is
> ceremony. Letting the runtime reclaim `World`'s parts is convenient and
> is exactly the affine hole §4 refuses elsewhere."*
>
> The affine-hole argument is correct and it is not the interesting
> one. Even if reclaiming were *safe* it would be wrong, because those
> four lines are the only place a program's authority surface is written
> down. §1 is that argument; §2 is the tool that follows from it.

---

## 1. The ceremony is the declaration

Measured first. Across this repository there are **290 `release` calls,
and all 290 are inside `main`.** Not one appears anywhere else. So the
cost does not compound with the size of a program — it is a fixed five
lines in one function, paid once per program rather than once per
abstraction.

And those lines say something. `release(fs)` on the third line of
`main` is a program stating *"I will never touch the filesystem"* —
checked, not claimed, because the capability is gone and nothing can
conjure another. The four releases of unused capabilities are not
ceremony around the program's authority; **they are the program's
authority declaration.**

Deleting them to save four lines deletes the declaration. The runtime
would reclaim the capabilities silently, every `main` would look alike,
and the one fact a reader of a capability-typed program most wants —
what can this thing reach? — would be nowhere.

### 1.1 Why `main` cannot say it in its type

The natural objection is that the row should carry this. It cannot, and
the reason is structural rather than an oversight.

§8.2: a row lists what a function **borrows**. Owning a capability is
strictly stronger and strictly more visible, so an effect a function has
outright does not appear in its row. That is why `main` prints while
declaring `[]`.

Which means every entry point in this repository has the same signature:

```
fn main(world: World) -> [] int
```

Fourteen examples, one signature, zero information. The type of `main`
is the one place the row model has nothing to say, because `main` is the
one function that owns rather than borrows. So the surface lives in the
body, as the pattern of `release` calls, and §1 is why that is the right
place for it rather than a consolation.

---

## 2. So compute it

If the declaration is in the body, a tool should read it:

```sh
$ lex-sys authority examples/tally.ls --std
performs
    io_read
    io_write
never touches
    the filesystem
    the heap
    the command line
    foreign code
```

The surface is the **union of what every function `main` reaches
performs**, and that needed no new analysis. Pass 2 already emits
exactly what `main` reaches (`standard-library.md` §5.2), so
`Program::funcs` *is* the reachable set. The same reachability that
decides what goes in the binary decides what the binary can do — one
fact, not two.

Rows are exact in both directions (§7.3), so this is **precise rather
than conservative**: a label here is an effect the program performs on
some path, not one it might.

### 2.1 What building it found

The first version unioned the **declared** rows, and it was wrong in
exactly the way §1.1 predicts. `examples/lines.ls` reads `argv` in
`main`'s own body, `main` declares `[]`, and the report said *never
touches the command line* about the repository's command-line tool.

So a function now carries what it **performs** as well as what it
declares — the set before ownership discharges it, which the checker
computed anyway and threw away. For every function that borrows its
authority the two are equal; for the one that owns, they are not, and
that one is the entry point. The bug and the section are the same
observation arriving twice.

### 2.2 The negative half

A capability language is for the question *what can this **not** do*, so
the report answers it. An absent label is a proof, not an absence of
evidence: the capability was released, and nothing in the language
creates another.

### 2.3 It shows narrowing

```
examples/tour.ls     fs_read("/tmp")  fs_write("/tmp")
examples/lines.ls    fs_read("")      fs_write("")
```

Both touch the filesystem; only one is confined to a directory.
`lines.ls` is unnarrowed for a reason it explains — a tool reading a
path the *user* chose has no literal to narrow to — and the point here
is that the difference is legible **from outside the program**, without
opening either.

---

## 3. What is still open

| Question | Why it waits |
|---|---|
| `release(a, b, c)` | Would make the five lines two while keeping every name, so it loses nothing §1 defends. It is also a variadic form in a language with none, for a saving of three lines once per program |
| ~~A machine-readable form~~ | **Done** — `--output json`, in the shape `lex-os-check`'s `CheckReport` already uses. `docs/budget.md` §5 is why it was the half actually wanted |
| Authority of a *library* | With no `main` there is no program, so there is no surface — only per-function rows, which `lex-sys ids` already lists |

---

## 4. The suite

| Test | Claim |
|---|---|
| `the_authority_report_names_what_a_program_performs` | §2, over three examples whose surfaces differ |
| `the_authority_report_sees_what_main_does_itself` | §2.1 — the bug: `lines.ls` reads `argv` in `main` |
| `an_unused_capability_never_appears` | §2.2, the negative half |
