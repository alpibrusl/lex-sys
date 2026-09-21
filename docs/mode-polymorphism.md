# Mode polymorphism

> **Status: settled, and it began by finding a leak and a double free.**
>
> §12 of `linearity-and-effects.md` lists mode polymorphism as open and
> *half-answered*: monomorphisation already makes `fn id[T](x: T) -> T`
> work at both modes. Checking that claim turned up a soundness hole
> underneath it, and the fix and the missing feature turn out to be one
> mechanism.

---

## 1. What was already true

More than the documents said. A generic container works at **both modes
in one program** today:

```
res struct Holder[T] { held: T, tag: int }

fn wrap[T](value: T, tag: int) -> [] Holder[T] { ... }
fn unwrap[T](h: Holder[T]) -> [] T { ... }

let boxed  = wrap(Ticket { serial: 5 }, 1);   // res
let number = wrap(41, 0);                     // val
```

Both instantiations compile, and each copy is checked at the type it was
instantiated at. So `Option[T]` and `Result[T]` over a resource type are
**expressible**, and have been since M2.

> A first version of this paragraph said `Vec[T]` too, and that was
> wrong — the third wrong answer in a row about the same question, each
> one looking at the generics. A `Vec` keeps its elements in a boxed
> slice and a boxed slice holds `val` data only, for reasons that have
> nothing to do with mode polymorphism. `docs/collections.md` §2.

`docs/standard-library.md` §4 said otherwise — that a `Vec[T]` "wants
generics over a mode" and would "promise more than it delivers". The
diagnosis was wrong, written from §12's open list rather than from a
test; the conclusion about `Vec` happened to be right, for a reason
neither document had found yet. They are all in `std` now except the
one that cannot be — §5.

---

## 2. The hole

A generic aggregate declared **`val`** is trusted rather than checked:

```
val struct Wrap[T] { held: T }

let w = Wrap { held: box(h, 41) };   // and nothing has to consume it
```

That compiles. Under valgrind: *8 bytes in 1 blocks are definitely
lost.*

It gets worse, because `val` means **copyable**:

```
let a = unwrap(w);
let b = unwrap(w);        // `Wrap[Box[int]]` is `val`, so `w` copies
unbox(h, a); unbox(h, b); // one allocation, two frees
```

*Invalid free() / delete / delete[] / realloc().* Ordinary code, no
`unsafe` anywhere — which this language does not have. The same shape as
the double free in `boxed-slices.md`, found the same way: by testing a
claim instead of believing it.

### 2.1 Why

`mode_of` returns a declared mode without looking at the members:

```rust
if let Some(declared) = defs[index].declared_mode {
    return declared;           // `val` stops here
}
// members are substituted and walked only below
```

And the declaration-time check that refuses *"`Rc` is declared `val`,
but it holds `Box[Cell]`, which is `res`"* runs against the members **as
written**, where `T` is `Param(0)` — which `mode_of` calls `val`. So the
declaration passes, the instantiation is never re-checked, and
`Wrap[Box[int]]` is `val` by assertion.

The boundary is exactly the `val` keyword. Without it the mode is
*computed* from the substituted members and comes out `res`, correctly;
`res struct Wrap[T]` is correct too. Only the explicit `val` is believed.

---

## 3. The fix is the missing feature

`val struct Wrap[T]` is a claim about **every** `T`. Written out, it
means:

```
val struct Wrap[T: val] { held: T }
```

So the fix is to have that bound, and to check it where a type argument
is supplied. Which is also what §12 was asking for — *"a signature that
says so and is checked once"*.

### 3.1 The rule

> A type parameter is bounded `val`, or it is unbounded. An unbounded
> parameter is checked as though it were **`res`**.

`res` is the stronger obligation: a body that satisfies linearity for a
linear `T` satisfies it for a copyable one. So a function checked once
under `T: res` is safe at **every** instantiation, and its errors land
on the definition, where the mistake is.

`[T: val]` is the opposite promise — this only works for copyable types
— and an instantiation at a resource type is refused **at the call
site**, naming the bound.

### 3.2 There is no `[T: res]`

It would mean "checked assuming `res`", which is what unbounded already
means. A bound that changes nothing is a keyword to explain and never
reach for, so it does not exist.

---

## 4. What that moves

The error for a generic that drops its parameter moves from the
instantiation to the definition:

```
fn sink[T](x: T) -> [] int { return 0; }
```

Before: accepted here, refused at `sink(open(5))` as *"instantiated at
`File`"*. After: refused **here**, because it is wrong for some `T` and
the definition is where that is true. Writing `[T: val]` is how a
program says it meant only copyable types, and then the refusal is at
the call site instead.

That is the whole trade, and it is the right way round: a library ships
its definitions, and an error inside `std/` pointing at code the caller
cannot change is the worst place for it.

### 4.1 What it costs in this repository

Three functions need `[T: val]`, and each is genuinely val-only:

| | |
|---|---|
| `unwrap_or` in `examples/tour.ls`, `examples/rational.ls`, `tests/accept/generics.ls` | Drops `fallback` on the `Some` path |
| `is_ok` in `examples/rational.ls` | Matches and ignores the payload |

They are not workarounds. `unwrap_or` over a resource type would have to
drop one of two values, which is the thing this language refuses — and
until now nothing in the signature said so.

---

## 5. What went into `std`

Not held up by mode polymorphism, which §1 shows was never the blocker.
`std.option`, `std.result`, `std.list` and `std.vec` are
`docs/collections.md`, and the bound did two things for them.

`unwrap_or` says `[T: val]`, because it drops one of two values — which
is the "place to put the value that is not returned" this section first
asked about, answered by saying out loud that there isn't one for a
resource.

And `res struct Vec[T: val]` needed the bound on a **type**
declaration, which §6 had listed as harmless-but-unnecessary. It is
neither.

---

## 6. Open

| Question | Why it waits |
|---|---|
| ~~`Option[T]` / `Result[T]` in `std`~~ | **Done** — `docs/collections.md`, along with `List` and `Vec` |
| ~~Bounds on a generic *type*'s parameters~~ | **Done, and the reasoning here was wrong.** It is true that `val struct X[T]` implies `T: val`, and that is the *only* case where it does: a `res` aggregate promises nothing about its parameters, and `res struct Vec[T: val]` is exactly what a vector needs — it owns an allocation, its elements are copyable. Writing the bound is refused on a `val` declaration and required on the others. `collections.md` §3 |
| Effect polymorphism | A function generic over the *row* it performs. Named nowhere yet, and much larger |

---

## 7. The suite

| Fixture | Rule | § |
|---|---|---|
| `val_generic_holding_a_res.ls` | The leak — a `val`-declared generic at a resource argument | 2 |
| `val_generic_copied.ls` | The double free, from the same hole | 2 |
| `unbounded_generic_drops_its_parameter.ls` | An unbounded parameter is checked as `res`, at the definition | 3.1, 4 |
| `val_bound_violated_at_the_call_site.ls` | `[T: val]` refused where the argument is `res`, and reported *there* | 3.1 |
| `res_bound_is_not_a_thing.ls` | There is no `[T: res]` | 3.2 |

| Accepting | Shows |
|---|---|
| `mode_polymorphism.ls` | One generic used at both modes, and a `[T: val]` one used at a copyable type |
| `tests/accept/generics.ls` | Rewritten: `unwrap_or` declares `[T: val]`, which is what it always meant |
