# Collections

> **Status: settled and built, and the answer is about *shape* rather
> than about generics.**
>
> The README's "still missing" line read: *a writer abstraction, and
> `Option`/`Result` over resources — neither blocked by the language any
> more, both wanting a library design.* Half of that was right. Writing
> the library found that the **linked** collection holds resources and
> the **array** one cannot, for a reason no API design gets around, and
> that saying so out loud needed a bound the language did not have.

---

## 1. What was already true

`mode-polymorphism.md` §1 established that a generic container works at a
resource type and a copyable one in the same program, and has since M2.
That holds for every collection here:

```
enum Option[T]     { None, Some(T) }
enum Result[T, E]  { Ok(T), Err(E) }
enum List[T]       { Empty, Cons(T, Box[List[T]]) }
```

`Option[Ticket]` is a resource the checker will not let a program forget.
`Option[int]` is copyable. Neither declaration writes a mode keyword,
because the mode is **computed** from the argument — and computing it is
what makes one declaration serve both.

`Result[Ticket, int]` is the case worth naming: two parameters at two
different modes in one type. The type is a resource either way, because
the mode is a fact about the **type** and not about which variant a
particular value happens to be in. A program holding a
`Result[Ticket, int]` that turned out to be `Err` still has to consume
it — and consuming it is a `match`, which costs nothing.

So three of the four modules here needed no language change at all. They
needed someone to write them and check the claim.

---

## 2. The array cannot hold a resource

`Vec[T]` is the one that does not work, and the reason is not the
generics. It is that a `Vec` keeps its elements in a **boxed slice**, and
a boxed slice holds `val` data only. Two independent reasons, either of
which alone would be enough:

**The fill is copied into every element.** `box_slice(h, n, fill)` is one
allocation holding `n` copies of `fill`, and a linear value cannot be
copied at all. There is no uninitialised alternative to reach for:
`defined-behaviour.md` has no uninitialised memory in it.

**Freeing the run runs nothing.** `unbox_slice` is one `free`. It does
not walk the elements, because there is nothing to walk them *with* —
this language has no destructors, and `heap.md` says so as a design
decision rather than an omission. An obligation sitting in element 3
would be dropped rather than discharged, which is precisely the affine
hole §4 of `linearity-and-effects.md` refuses everywhere else.

The second reason is the one that closes the question. The first looks
like an API problem — hand `empty` a list of elements instead of a fill,
and it goes away. The second does not: however a `Vec[Ticket]` were
built, ending it would have to end every ticket, and nothing in the
language can be asked to do that.

### 2.1 And the list can

A list is a chain of boxes, and `unbox` hands back **what the box held**.
So taking a list apart *produces* its elements, one at a time, and a
program that wants to end them has each one in its hand as it goes. The
walk that reads the list is the walk that frees it, which
`tests/accept/linked_list.ls` has said since M2 — what is new is only
that the list is now generic, so it says it for `List[Ticket]` too.

One allocation per element is the price, and it is a real one. What it
buys is the only container a program can put a resource in.

| | Over `val` | Over `res` |
|---|---|---|
| `Option[T]`, `Result[T, E]` | yes | yes |
| `List[T]` | yes | **yes** |
| `Vec[T]` | yes | **no**, and not for want of a feature |

---

## 3. So the bound goes on the declaration

`std.vec` has to *say* this, and saying it was the one thing the language
could not do:

```
res struct Vec[T: val] { held: Box[[T]], used: int, fill: T }
```

The vector **owns** an allocation, so it is `res`. Its elements are
copyable, so `T` is `val`. Two modes in one declaration, about two
different things — and a `val`/`res` keyword cannot express it, because
the keyword speaks about the aggregate while this is about a parameter.

Before this, a bound on a type declaration was refused outright, on the
grounds that `val struct Wrap[T]` already means `T: val`. That reasoning
is correct and it is not general:

- A **`val`** aggregate is `val` at every instantiation, which can only
  hold if every argument is. So saying `val` *is* the bound, and writing
  it again is a second way to say one thing. Still refused.
- A **`res`** aggregate promises nothing whatever about its parameters.
- An **undeclared** one has its mode computed from them, so a bound is a
  restriction on what may instantiate it rather than a restatement.

In the last two the bound says something new, so it is written. This is
`mode-polymorphism.md` §6's second open row, answered — and answered the
other way from how that row guessed, which had only the `val` case in
view.

There is no `[T: res]`, on a type any more than on a function: an
unbounded parameter is already checked as `res`, so a `res` bound would
change nothing (`mode-polymorphism.md` §3.2).

### 3.1 It reaches the hash

A bound narrows what may instantiate the declaration, exactly as
narrowing a field's type would, so two declarations differing only in a
bound accept different programs and must be two types. Not through
`mode_tag`, which reads an absent mode as `val`: that is right for a
declaration and wrong here, since unbounded is the **stronger** check.

### 3.2 And it is kept at the use site

`Vec[Ticket]` is refused where the caller wrote `Vec[Ticket]`, naming the
bound. That placement is the whole argument for having the bound at all.
Without it the program would still be refused — a boxed slice holds `val`
data only — but the refusal would land inside `std/vec.ls`, pointing at a
`box_slice` call the caller neither wrote nor can change. An error inside
a library is the worst possible place for one.

---

## 4. A generic function can move a resource, and can never end one

This is the rule the whole library is shaped by, and it falls out of
having no way to speak about a `T` except by moving it.

`list.push` moves a `T` into a node. `list.pop` consumes the list, frees
exactly the one node it took apart, and hands the element **back**:

```
pub fn pop[T, &h](heap: &!h Heap, list: List[T]) -> [heap] Option[(T, List[T])]
```

It discharges nothing. After it returns, the caller owes the `T`. A
`pop` that dropped the element instead would have to be `[T: val]`, and
then there would be no way to get a resource out of a list at all.

So `list.drop` — which ends every element — is `[T: val]`, and over a
resource the caller writes the drain itself. That is not a gap in the
library so much as the language observing that **only the owner of a
`Ticket` knows what ending one means**. `examples/queue.ls` is that
loop, eight lines long, and the job it forgets to finish is a compile
error rather than a leak.

### 4.1 The drain is recursive, and that is forced

A `while` loop that pops until empty leaves a `List[T]` behind, and the
checker cannot see that it is empty — emptiness is a fact about the
value, not about the type. So the residual still carries the obligation,
and consuming it needs the same drain, which is where it came in.

Recursion has no residual: the base case is `Empty`, and a `match`
consumes that outright. Every drain in this repository is written that
way, and it is worth knowing it is a requirement rather than a taste.

### 4.2 The bug this turned up

Keeping a declaration's bound means reading an argument's mode, and it
read it against **nothing** — so a rigid `T` came out `res` however the
enclosing function had bounded it:

```
val struct Wrap[T] { held: T }

fn rewrap[T: val](w: Wrap[T]) -> [] T { ... }
    error: `Wrap` is declared `val`, so its type arguments are `val`
           too, and `T` is `res`
```

`T` is bounded `val` on the line above. A `[T: val]` function could not
name a `val` aggregate at `T` at all, which made the bound unusable in
exactly the position it exists for. The mode of an argument is now read
against the bounds of the declaration the type is written **inside**.

---

## 5. What the module system still owed

`Pattern::Variant` carried no qualifier. An enum could be *named*
through an import, held, and passed around — and never taken apart,
because a `match` could only name an enum its own module declared.

That is fatal for a library of enums. `std.option` would have been a
type a program could hold and never open. So a pattern takes the same
qualifier every other reference does:

```
match list.pop(heap, queue) {
    option.Option::None => { ... }
    option.Option::Some(pair) => { ... }
}
```

A module still reaches no hash: what gets encoded is the referenced
declaration's hash, which the qualifier is only used to look up
(`modules.md` §2). The same `match` written flat and written through an
import is one body, and there is a test.

### 5.1 The asymmetry that is still open

`match` on a reference **borrows**: a reference gives references
(`reading-references.md` §2). Field access on a reference **copies**,
which is why it is refused for a `res` field. The consequence is that a
struct with a `res` field cannot be read through a reference at all,
while the equivalent enum can.

Two library modules have now paid for this. `std.buffer`'s `write` takes
the buffer by value and hands it back; `std.vec`'s `get` does the same,
and needs a stored `fill` besides, because a generic function cannot
*name* a `T` to seed a `var` with before the `borrow` that fills it in.
That the bound makes keeping a spare `T` free is a nice accident, not a
design.

This is the next thing to settle here, and it is a change to
`reading-references.md` rather than to this document.

---

## 6. The modules

| Module | Holds a resource? | What the mode decides |
|---|---|---|
| `std.option` | yes | `unwrap_or` is `[T: val]` — it drops one of two values |
| `std.result` | yes, in either position | `unwrap_or` is `[T: val, E: val]`; two bounds, each earned by a different line |
| `std.list` | **yes** | `push`/`pop`/`length` are unbounded; `drop` is `[T: val]` |
| `std.vec` | no | the whole declaration is `[T: val]` |

`is_some`, `is_ok` and `length` take a reference, which is what lets them
work over a resource: reading an `Option[Ticket]` that way costs the
caller nothing, where taking it by value would hand over a ticket the
caller still owes.

---

## 7. Open

| Question | Why it waits |
|---|---|
| Reading a `res` field through a reference | §5.1. A change to `reading-references.md`, and the second module to pay for it is the point at which it stops being hypothetical |
| A writer abstraction | Still missing, and still unaddressed by this slice. It wants either closures or a dispatch story, and the effect row of a writer that could be a file *or* the console is the interesting part |
| `map` / `and_then` on `Option` | No closures, so there is nothing to pass |
| A hash map | Wants `Vec`, so it inherits §2 — a map holding resources is the same wall |

---

## 8. The suite

| Fixture | Rule | § |
|---|---|---|
| `generic_array_over_a_resource.ls` | The array is refused at the **definition**, since unbounded is checked as `res` | 2 |
| `val_declaration_rebounds_its_parameter.ls` | A `val` aggregate already bounds its parameters | 3 |
| `res_bound_on_a_type.ls` | There is no `[T: res]` on a type either | 3 |
| `type_bound_violated.ls` | The bound is kept where the type argument is supplied | 3.2 |

| Accepting | Shows |
|---|---|
| `tests/accept/collections.ls` | `Option`, `Result` and `List` over a resource; `Vec` at a copyable element; and §4.2's `[T: val]` function naming a `val` aggregate |
| `examples/queue.ls` | Jobs that own memory, held in a list, ended exactly once each — and the tally beside them in a `Vec[int]` |

| Test | Claim |
|---|---|
| `a_bound_on_a_type_parameter_reaches_its_hash` | 3.1, and that the bound is per parameter |
| `a_res_aggregate_may_bound_its_parameters` | 3, both halves |
| `a_bounded_parameter_may_stand_where_a_val_aggregate_wants_one` | 4.2, the bug |
| `a_match_names_an_enum_through_its_qualifier` | 5, and that the qualifier is checked rather than decorative |
| `qualifying_a_pattern_reaches_no_hash` | 5, and `modules.md` §2 still holding |
