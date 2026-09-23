# Function values: the shape, settled before anything forces it

> **Status: a documented "not yet", with every decision written down.**
>
> The audit's L1 asked for this document *before* anything forced it.
> `Rule::NoFunctionValues` refuses function values, and a sort
> comparator, an iterator, a callback, `pthread_create` and a plugin
> interface all want one. The audit called this "the decision most likely
> to force changes to existing row semantics", so it should be settled
> while the vocabulary is still moving.
>
> Counted by reading, **no program here asks**. There are 594 function
> bodies, and the only pair that differs by nothing but its callee is
> `io.write_all` against `io.error_all`, which
> [`effect-polymorphism.md`](effect-polymorphism.md) §3 had already found.
> The one sort has one comparator. None of the 22 enums picks an
> operation.
>
> The design question has a firm answer, and it is the useful part.
> **Function values do not force row variables, and they change no
> existing row rule.** A captureless function value whose type carries a
> fixed row is checked exactly as a named call is. What *would* change the
> row semantics is abstracting over rows, and that is a separate feature
> that has already been answered no.

---

## 1. What was asked

The audit, L1:

> Function values are refused (`Rule::NoFunctionValues`). […] Write the
> design doc **before** anything forces it. Decide whether closures
> exist, what mode a closure capturing a `res` has, and how a
> higher-order row is spelled. A captureless `fn` pointer with a fixed
> row may be enough and keeps the checker total.

That is three questions and one candidate. §4 answers the three, and it
takes the candidate, with two conditions the audit did not name.

---

## 2. The askers, counted

Every function body in `std/`, `examples/`, `benches/` and
`tests/accept/` was counted by printing each file canonically and
splitting it at top-level `fn`: **594 bodies** before this document's
fixtures were added. `tests/reject/` is left out, since those programs
are written to fail.

**Exact near-duplicates.** Each body was reduced to a shape in which
every name and literal becomes `ID`, except a name in call position,
which is kept. Six groups have the same shape but different callees:

| Group | What it is |
|---|---|
| 6 `main`s | The `split`/`release` preamble, differing in the one function each calls |
| 3 pairs across `tour.ls` and `match_a_reference.ls` | The same recursive walks, differing only in their **own** names |
| `show` / `emit` | Two one-line printers in different files |
| **`io.write_all` / `io.error_all`** | **The one real candidate** (§3) |

**Loose near-duplicates.** A second pass also made binary operators
interchangeable and ignored `main`, looking for the same loop with a
different operation inside it. That is where a function value usually
earns its place. It found four groups, and every one is either a
function copied between `examples/` and `std/` or a recursion that
differs only in its own name.

**Sorting.** `examples/sort/` is the only sort. It has one comparator,
`before`, with one caller. It takes no flags, so nothing picks an
ordering at run time.

**Dispatch already written by hand.** There are 22 enum declarations,
and every one carries data (`List`, `Option`, `Tree`, `Step`, `Arg`, …).
No `match` anywhere selects an *operation*, which is how a first-order
language spells a function value (§3).

**Callbacks into C.** No program declares an `extern fn` that takes a
function pointer. The API that `reach.md` §3.3 names, `pthread_create`,
is refused on other grounds before a function value would matter (§5).

So by the bar [`CONTRIBUTING.md`](../CONTRIBUTING.md) sets, two
askers, the count is **zero**. There is one near-miss, and §3 shows that
function values do not help it.

---

## 3. What the language writes instead, and what it costs

lex-sys compiles whole programs. So the set of functions a value could
name is always known, and an `enum` with one variant per function plus a
`match` that calls one can always say the same thing. This is Reynolds'
**defunctionalization**, and it needs nothing the language lacks.
`tests/accept/defunctionalized_stream.ls` collapses `write_all` and
`error_all` that way:

```
enum Stream { Out, Err }

fn write_to[&r, &i](io: &!i Io, which: Stream, s: &r [byte]) -> [io_write, err_write] int {
    match which {
        Stream::Out => { return write_bytes(io, s); }
        Stream::Err => { return write_err(io, s); }
    }
}
```

It compiles, runs and prints. **The cost is in the row.** `write_to`
performs whatever any of its arms performs, so every caller has to
declare both streams, including a caller that only ever passes `Out`.
`tests/reject/defunctionalized_row_is_the_union.ls` is that caller with
its row narrowed to `[io_write]`, and it is refused (`effect-not-declared`).
The authority report of such a program says it writes standard error
when it never does.

This cost is specific. It appears only when the arms have **different
rows**. A comparator is `[]` in every arm, so defunctionalizing a sort
costs nothing: `sort -r -n` would take a `bool` or an enum, not a
function value.

Would a function value recover the precision? That is the question the
audit is really asking, and §4.3 answers it: not unless `write_to` can
also be generic over the row.

---

## 4. The decisions

### 4.1 No closures

A closure is a function value that *captures*, and every hard question
in L1 is about captures:

- **A captured `res`** makes the closure itself `res`, callable once and
  obliged to be called or released. That is Rust's `FnOnce`, and it
  would follow from [`linearity-and-effects.md`](linearity-and-effects.md)'s
  rules without a new one.
- **A captured reference** gives the closure that reference's region, so
  it could not outlive the borrow. That is also a consequence of existing
  rules, not a new one.
- **A captured capability** is the reason for the "no". Today authority
  reaches a function only through its parameters, so a signature lists
  every capability the body can use. Every narrowing argument in
  [`reach.md`](reach.md) reads a signature that way. A closure that
  captured `&!i Io` would carry authority that no parameter names. Its row would still say `io_write`,
  so the *effect* would stay visible, but the capability would have
  travelled without passing through a parameter list.

So the answer to "what mode does a closure capturing a `res` have" is
written down (`res`, called once), and closures stay out. Nothing asks
for them (§2), and the one thing they add over §4.2 is hidden authority.

### 4.2 Function values, when an asker arrives

This is the audit's candidate, with its conditions made exact:

| | Rule | Why |
|---|---|---|
| What can be a value | A named, top-level function that declares **no type parameters** | Monomorphisation needs every instantiation to be known where the value is made. A call takes its type arguments from its arguments and never from the expected type (*"cannot tell what `T` is in this call […]; it is not determined by the arguments"*), and a function value has no arguments to take them from |
| Its type | `fn(A, B) -> [row] R`, with the row **written and exact** | The row stays where `linearity-and-effects.md` §7.2 put it: at a boundary and never inferred across one |
| Its mode | `val` | It captures nothing, so copying it copies no obligation |
| Its region | None of its own | The code is static. Only the regions its *type* mentions matter |
| Regions in the type | Only regions already in scope where the type is written | So `sort_by[&t](xs: &t [T], less: fn(&t [byte], &t [byte]) -> [] bool)` is fine, and a type quantified over its own regions (rank 2) is refused |
| A call through it | Performs exactly the row in its type | The same check as a named call, with the row read from the type instead of the declaration |
| Identity | A new expression node naming the target's `DefId`, hashed through the target's `SigId` | The same edge a call already has. A program that writes no function value hashes exactly as it does today |

### 4.3 Row semantics do not change, and row variables do not arrive

`linearity-and-effects.md` §10 made a prediction: *"When closures arrive
they bring row variables with them."* For function values as §4.2 shapes
them, **they do not.**

- A call through a value is checked against a declared row, the one in
  the value's type, exactly as a named call is checked against the
  callee's declaration. The check stays one walk of the body.
- A higher-order function writes down the row of what it calls, because
  that row is part of the parameter's type. So its own row is still
  written, exact and constant.

What this buys is less than it looks, and §3's example shows exactly
how much. Try to recover `write_to`'s precision with a function value:

```
fn write_with[&r, &i](io: &!i Io, w: fn(&!i Io, &r [byte]) -> [io_write] int,
                      s: &r [byte]) -> [io_write] int
```

This accepts `write_bytes` and **not** `write_err`, because the row is
fixed in the parameter's type. Accepting both needs `write_with` to be
generic over that row, which means a row variable. That is
[`effect-polymorphism.md`](effect-polymorphism.md)'s feature, and it
found nothing to quantify over. Without it, fixed-row function types
give one higher-order function per row, which is no better than today's
two wrappers.

So the dependency runs one way. Function values do not need row
variables. The *precision* people expect from function values does, and
that is a separate decision, already made.

---

## 5. Callbacks into C, the one thing no enum can write

Defunctionalization works because this program can see every function
the enum might name. C cannot see the enum, so a function pointer handed
to C is the one thing §3 cannot replace. Three conditions settle what is
reachable:

1. **The row must be `[]`.** C calls the function with C's arguments, and
   C cannot supply a capability, since there are no captures (§4.1) and
   no context pointer to carry one (condition 2). A callback that
   performed `io_write` could never have been handed the `Io` to do it
   with.
2. **Its parameters must be scalars.** Most callback APIs pass `void *`,
   and a pointer from C carries no region. That is the same wall as
   [`reach.md`](reach.md) §3.1, which blocks TLS and libpq. `qsort` and
   `bsearch` compare `const void *` arguments, and `pthread_create`'s
   start routine takes and returns one.
3. **The calling convention already matches.** Every lex-sys function
   is emitted with the target's default convention
   (`module.isa().default_call_conv()` in `lex-sys-codegen/src/emit.rs`),
   which is the C ABI for scalar arguments.

What is left is `atexit` (`void (*)(void)`) and `signal` (`void (*)(int)`).
**No program here asks for either.** Threads are not unlocked by
function values: `pthread_create` fails condition 2, and a thread also
needs an answer about data races that this document does not give.
`reach.md` §6 said that threads want "more than a pointer", and this is
the list of what more.

---

## 6. What would create an asker

Not a prediction. This is the list to check against, so the next reader
does not have to re-derive it. The bar is two programs.

| Asker | Why it would count |
|---|---|
| A real program that needs `atexit` or `signal` | §5's reachable remainder. A graceful `SIGINT` in `examples/serve/` is the likeliest first one |
| A library compiled separately from its callers | Defunctionalization needs the whole program in view. This language has never compiled anything else ([`many-files.md`](many-files.md)), so this would be a change to that first |
| Arms with **different rows** where the precision matters | §3's cost. But recovering it needs row variables as well (§4.3), so this asker is really an asker for `effect-polymorphism.md` |

A second comparator, from `sort -r` or `sort -n` through
[`flags.md`](flags.md), would **not** count. Comparators are pure, so an
enum costs them nothing (§3).

---

## 7. What this does not say

- **Not that function values are wrong.** §4.2 is a design that could be
  built as written. The claim is only that nothing here needs it yet,
  and that building it would not disturb the row system.
- **Not that `Rule::NoFunctionValues` is permanent.** It is the current
  answer to a counted question. Its fixture,
  `tests/reject/function_as_value.ls`, stays. The rule's message still
  names "M1", a milestone it has outlived, and this slice leaves it alone
  because `reach.md` §3.3 quotes it word for word.
- **Not a claim about higher-order code in general.** In a language with
  separate compilation, or with an effect system that infers rows,
  §3's substitute would not be available and the arithmetic would be
  different.

---

## 8. The suite

| Fixture | What it pins |
|---|---|
| `tests/accept/defunctionalized_stream.ls` | §3: an enum and a `match` write what a function value would, today, with no new feature |
| `tests/reject/defunctionalized_row_is_the_union.ls` | §3: the cost, since a caller of the enum version cannot declare less than the union of the arms' rows (`effect-not-declared`) |
| `tests/reject/function_as_value.ls` | The rule this document leaves in place (`no-function-values`) |
