# Sharing

> **Status: settled and built, and it corrects the document it implements.**
>
> §9 of `linearity-and-effects.md` is the last unbuilt item on M2's list.
> It names two escape hatches, `Rc` and `Gen`, and says of both:
>
> > *"both **libraries, not language features** — neither touches the
> > checker, and that is the point."*
>
> That is true of `Gen` and **false of `Rc`**. This document is what
> building them found, and §9 now points here.

---

## 1. What §9 was for

Linearity says a value has exactly one owner. That is most of why this
language can promise no leaks, no double frees and no use-after-free
without a garbage collector or a borrow checker.

It also cannot express a graph, an observer, or anything where "who owns
this" has no answer. §9 is the admission that such structures exist and
the plan for reaching them: two libraries, paying a documented runtime
cost, neither of which the checker has to know about.

The plan was right about the cost and about `Gen`. It was wrong about
whether `Rc` can be written at all.

---

## 2. `Rc` is not expressible, and that is not a small thing

`Rc` needs **N owners of one allocation**. An owner has to be a value, so
the question is what value could refer to that allocation. This language
has three, and none of them works:

* **`Box[T]`** is `res` — used exactly once. Two of them referring to one
  allocation is precisely what linearity forbids.
* **`&r T`** is bounded by its region. An `Rc` outlives any block, and a
  reference cannot.
* **`&static [byte]`** outlives everything, and there is no way to make one
  except a literal.

So there is no **copyable pointer**, and `Rc` is a copyable pointer with a
count attached.

### 2.1 Three ways to try, three different refusals

This was not reasoned out in the abstract. Each shape was written and
compiled, and the compiler refused each for its own reason:

| Attempt | Refusal |
|---|---|
| `clone` from a borrow — `Rc { held: rc.held }` | *"`Box[Cell]` is `res`, and nothing moves out of a reference"* |
| Consume one and hand back two | *"`held` has already been consumed; a `res` value is used exactly once"* |
| Declare `Rc` itself `val` so it copies | *"`Rc` is declared `val`, but it holds `Box[Cell]`, which is `res`"* |

Three rules, none of which was written with `Rc` in mind, and each of
which is individually right. Together they say the same thing: **nothing
here copies a pointer.**

### 2.2 What it would take, and why not yet

A `val` type that holds a heap address. That is a raw pointer, and adding
one would put back exactly the hole this language spent M2 closing: a
copyable address is a use-after-free waiting for the allocation to end.

It could be done safely — a pointer whose *only* consumer is a checked
operation, with the count in the allocation — but that is a language
feature with its own document, not a library. §9's "neither touches the
checker" is the part that does not survive contact.

### 2.3 What you can have instead

Shared ownership is not impossible here; the **ambient handle** is. An
`Rc` whose operations thread a slab — `clone(slab, handle)`,
`release(slab, handle)` — is expressible today, because then the handle is
an index rather than a pointer and the slab is the one owner.

That is `Gen` with a reference count where its generation goes, which is
why §9's own advice — *"`Gen` is the one to prefer"* — turns out to be
stronger than it knew.

---

## 3. `Gen` is a library, exactly as promised

```
val struct Gen { index: int, generation: int }
```

Two plain `int`s. `val`, so it copies freely; and it **points at nothing**,
which is why copying it is safe. The `Slab` owns every value, is `res`, and
is ended exactly once like any other resource.

A handle is checked on every use, against the slab it was made for:

1. is the index in range?
2. is that slot live?
3. does its generation match the handle's?

Any of those failing gives `Missing` — **a value the program decides what
to do about.** A dangling pointer is undefined behaviour and the program
gets no say. §9 says that asymmetry "is most of why this language exists",
and it is the whole reason `Gen` is worth its cost.

### 3.1 What it costs

| Operation | Cost |
|---|---|
| `insert` | A scan for a free slot. A free list would make it O(1) and is a policy the library could choose |
| `get` | Two comparisons for the bounds, one for liveness, one for the generation, and a branch |
| `remove` | The same checks, plus one increment — which is what makes every outstanding handle to that slot stale at once |
| A handle | Two `int`s, copied like any other `val` |

The slab is one boxed slice (`boxed-slices.md`), so the values are
contiguous and the whole thing is one allocation and one `free`.

Generations are not recycled and `int` is 64-bit, so a slot would have to
be reused more times than a program can count before a stale handle could
collide with a live one. This document will not pretend that is a proof;
it is a bound, and it is the same one every generational-index library
relies on.

---

## 4. What building it found about the language

A library is a better test of a language than a test suite is, and this
one surfaced three things that made it more verbose than it should be.
None is a soundness problem and all three are ergonomics:

* **No tuples.** `insert` must answer with both a slab and a handle, so it
  returns a `res struct Inserted { slab, handle }` declared for the
  purpose. Every operation that threads the slab needs one.
* **No renaming in a destructuring pattern.** `let Slab { entries, live }`
  binds those names and no others, so two slabs cannot be taken apart in
  one scope.
* **No shadowing within a block.** Threading a value through several steps
  means `fresh`, `live`, `emptied`, `stale` — four names for one slab —
  because `let slab = ...` twice in a block is refused.

Together they make value-threading APIs wordy, which is the exact style a
linear language pushes you toward. They are listed in §6 rather than fixed
here, because each is a language change and this document is a library.

---

## 5. What this does not add

* **No `Rc`.** §2.
* **No raw pointers.** §2.2 — and that is the point, not an omission.
* **No generic `Slab[T]`.** The library is a slab of `int`, because
  monomorphised generics over a boxed slice work but the example is
  clearer without the extra parameter. Nothing stops a `Slab[T]`.
* **No free list.** §3.1: `insert` scans. Making it O(1) is a policy the
  library would choose, the way `buffer.ls` chose doubling.

---

## 6. Open

| Question | Why it waits |
|---|---|
| A safe copyable pointer, and a real `Rc` | §2.2. A language feature with its own document |
| Tuples, or multiple return values | §4. The single biggest ergonomic gap a linear language has |
| Renaming in patterns, shadowing in a block | §4, and both are small |
| A free list in the slab | Policy; the library can have one whenever it wants |

---

## 7. The suite

| Fixture | Rule | § |
|---|---|---|
| `rc_clone_from_a_borrow.ls` | Nothing moves out of a reference | 2.1 |
| `rc_cloned_twice.ls` | A `res` value is used exactly once | 2.1 |
| `rc_declared_val.ls` | A `val` type may not hold a `res` | 2.1 |

Three fixtures for one conclusion, which is unusual and deliberate: the
claim in §2 is that `Rc` fails *whichever* way it is attempted, and one
fixture would only show that it fails one way.

| Accepting | Shows |
|---|---|
| `examples/slab/` | `Gen` as a library: a handle used, the slot removed, and the same handle coming back `Missing` |
