# `defer`

> **Status: settled and built.**
>
> §4.2 of `linearity-and-effects.md` called `defer close(f);` "the obvious
> sugar" and deliberately left it out of M2, so the expansion and the
> checker would not be debugged at the same time. §12 kept one question
> open: *is a consumption the programmer did not write at the point it
> happens still "visible"?*
>
> §1 answers it. The rest of the document is smaller than the question.

---

## 1. Is it still visible?

**Yes, because "visible" here has never meant "written on the line where
it runs".**

The word does real work in this project, and it means something precise:
*the type says what happened.* §7's rows are exact so that reading a
signature tells you what a function did; §2.1 of
`reading-references.md` refuses inferred binding modes so that reading a
`match` tells you what a binding is. Both are about what a reader can
learn **without executing the program in their head**.

`defer` takes nothing away from either:

- The effect is still performed, so the row still declares it. A
  function whose `defer` closes a file still says `[fs_write]`, and one
  that hid an effect this way would not compile.
- The consumption is still checked. The expansion is real, so the
  exactly-once rule is enforced on every path — a value consumed twice
  is the ordinary "already consumed" error, and one consumed never is
  the ordinary leak.
- The line is still written. `defer close(f);` is text in the function,
  one line below the `open`.

What moves is *where the text sits*, and it moves to the better place.
Compare:

```
fn take(f: File, flag: bool) -> [] int {
    if flag { return close(f) + 2; }
    return close(f) + 4;
}
```

A reader checking this has to find every exit and confirm each closes
exactly once — work that grows with paths times resources. With the
consumption written next to the acquisition, there is one line to find
and the checker does the counting:

```
fn take(f: File, flag: bool) -> [] int {
    defer close(f);
    if flag { return 2; }
    return 4;
}
```

The pairing is what a reader is looking for, and `defer` puts the two
halves of it adjacent. That is *more* visible, not less.

### 1.1 What would have failed this test

A destructor. `~File()` running at scope end is invisible in three ways
`defer` is not: nothing is written at the use site, the effect does not
reach the row, and which function runs is decided by the type rather
than named. `heap.md` refuses those and this does not reopen them —
`defer` names the function, and the row still tells the truth.

---

## 2. The rule

> `defer E;` runs `E` at **every exit from the block it is written in**,
> in reverse order of declaration.

An exit is falling off the end of the block, or a `return` in it or
under it. There is no `break` or `continue`, so that list is complete.

A `return`'s value is evaluated **first**, then the defers run, then the
function returns. That order is forced rather than chosen: a
`defer close(f)` alongside `return fd_of(f)` has to read `f` before the
close, and the other order would make every such function unwritable.

`E`'s value is **discarded**, so it obeys the rule an expression
statement obeys: discarding a `res` is a leak, and `defer open(1);` is
refused for exactly that.

### 2.1 Block scope, not function scope

```
fn nested[&i](io: &!i Io) -> [io_write] int {
    defer putchar(io, 90);      // Z — the function body's frame
    if true {
        defer putchar(io, 89);  // Y — this block's frame
        putchar(io, 88);        // X
    }
    putchar(io, 87);            // W
    return 0;
}                               // prints XYWZ
```

Block rather than function, because `borrow` and `region` are blocks and
a `defer` inside one should run before that block closes — a defer that
outlived the borrow it was written in could not touch what it borrowed.

### 2.2 It does not run on a trap

A trap kills the process. There is no unwinding here and no
`defer`-runs-on-panic, because there is no panic: `defined-behaviour.md`
§2.1 stops rather than continuing with a wrong answer, and a stopped
process has nothing left to tidy.

---

## 3. It is sugar, and stays sugar

`defer` is expanded during **lowering**, into the statement it stands
for, once per exit path. Nothing downstream knows it exists: the linear
checker replays the same events it would have replayed for the hand
written version, and the backend emits the same code.

That is deliberate, and it is what §4.2 was waiting for. A `defer` that
the checker knew about would be a second set of linearity rules to keep
in agreement with the first. This way there is one set, and `defer` is a
way of writing it.

The expansion is re-run per path rather than shared, which is what makes
a double consumption the ordinary error:

```
defer close(f);
return close(f);
    error: `f` has already been consumed; a `res` value is used exactly once
```

The diagnostic points at the `defer`, which is right: it is the one that
runs second.

### 3.1 One hash, not two

`defer close(f);` and `close(f);` run at different points, so they are
different programs and get different `BodyId`s. The tag is the
statement's own, never the expansion's.

---

## 4. What it is worth

Measured rather than assumed. Across this repository the pattern §4.2
describes — one resource, several exits — appears in **twelve**
functions, and several of those would not benefit, because their paths
consume *different* things and rebuild (`examples/tree.ls::insert` is
the clearest).

The honest case is not the existing code; it is the code that was not
written. A fallible pipeline that acquires a buffer and bails out at
four checks repeats the drop four times, tangled into each return
expression. That program is `examples/pipeline_checks.ls`, and it is
seven lines shorter and considerably clearer with one `defer`.

`defer` does **not** help the ceremony at `main`. Five `release` calls
become five `defer release` calls, which is the same five lines —
`ROADMAP.md` keeps that question separate, and it is a different one.

---

## 5. Open

| Question | Why it waits |
|---|---|
| `defer` on a value that is later moved | Currently a plain "already consumed" at the defer's expansion. Correct, and the message could name the move instead |
| `errdefer` | Zig's run-only-on-failure form wants a notion of failure the language does not have: there is no unwinding, and a `Result` is an ordinary value |
| Running defers on a trap | §2.2. Would need unwinding, which is a much larger decision than this one |

---

## 6. The suite

| Fixture | Rule | § |
|---|---|---|
| `defer_consumes_twice.ls` | The expansion is real, so a second consumption is the ordinary error | 3 |
| `defer_leaks_its_value.ls` | The value is discarded, so it may not be `res` | 2 |
| `defer_on_a_frozen_value.ls` | A `defer` may not consume what an enclosing `borrow` froze | 2.1 |

| Accepting | Shows |
|---|---|
| `defer.ls` | LIFO order, block scope, a loop body, and an early return through two frames |
| `examples/pipeline_checks.ls` | §4: the program the feature is for |
