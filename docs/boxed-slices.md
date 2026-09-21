# Boxed slices

> **Status: settled, and built in the same change.** `heap.md` §5 deferred
> this with one sentence — *"Boxing an unsized referent means a box
> carrying a length, which is a second shape with its own rules"* — and §7
> listed it as open. This is those rules.
>
> It is the foundation every collection needs. A growable buffer is a
> boxed slice plus a copy, and `Vec`, a hash map, a slab and a string
> builder are all a growable buffer plus a policy.

---

## 1. What is missing

A run of values can live in two places today and neither is enough.

An **arena slice** (`alloc_slice[a]`) is bounded by its block. That is the
whole design of §6 and it is right for what it is for, but a collection
that outlives the function that built it cannot be one.

A **box** (`Box[T]`) escapes its block, which is the point of `heap.md`,
but it holds exactly one value of a **sized** type. `[T]` is unsized —
its length is a runtime value — so `Box[[T]]` was refused.

Between them there is no run of values whose lifetime the program decides.
So there is no `Vec`, no buffer that grows, and no collection at all.

---

## 2. The second shape

`Box[T]` at run time is a pointer and nothing else (`heap.md` §3.2).
`Box[[T]]` cannot be, because nothing else knows how many elements there
are:

> **A boxed slice is a pointer *and* a length** — two leaves, where an
> ordinary box is one. The box owns the length, because the length is
> what was allocated.

That is the same pair `&r [T]` already is, which is what makes §3's
dereference work without a new rule.

### 2.1 Elements are `val`, as everywhere else

Same rule as an arena (§6.1) and for the same reason: ending a boxed
slice frees memory and **runs nothing**, so a linear obligation put inside
one would be dropped rather than discharged. The error is the one arenas
already give, because it is the same rule rather than a new one.

---

## 3. Three operations

```
box_slice(h: &!x Heap, count: int, fill: T) -> [heap] Box[[T]]
unbox_slice(h: &!x Heap, b: Box[[T]])       -> [heap] int
contents(b: &r Box[[T]])                    -> [] &r [T]
```

`box_slice` allocates `count` elements and writes `fill` into each, the
way `alloc_slice` does — a loop, because the length is a runtime value,
which is what makes it a slice. A negative count traps, and so does a
`count * stride` that overflows: asking for less memory than you are about
to write is not a small allocation, it is a mistake.

**`unbox_slice` is a different operation from `unbox`, and has to be.**
`unbox` hands back what the box held; `[T]` is unsized, so there is
nothing to hand back. This frees the allocation and answers how many
elements it freed. It is the *only* consumer a boxed slice has, so §3.1 of
`heap.md` holds here unchanged: a boxed slice that is never unboxed is a
compile error, and the heap still cannot leak.

`contents` is the same `contents`, mode- and region-preserving, and needed
no new rule in the checker at all — a boxed slice's two leaves *are* a
`&r [T]`, so the dereference that reads a box reads this one too. Only the
backend had to learn that it is loading two leaves rather than one.

### 3.1 What it costs

| Operation | Cost |
|---|---|
| `box_slice(h, n, v)` | One `malloc`, plus `n` writes |
| `unbox_slice(h, b)` | One `free` |
| `contents(b)` | Two loads, a pointer and a length |
| `s[i]` through it | One bounds check, as every index has |

---

## 4. Growing is not an operation

There is no `grow`, `push` or `realloc`, and that is deliberate.

Growing a buffer is: allocate a bigger one, copy, end the old one. Every
part of that is already expressible, and writing it as a library rather
than a builtin means the *policy* — double each time? add a fixed
amount? — belongs to the program that chose it rather than to the
language.

`examples/buffer/` is that library: `buffer.ls` holds a growable byte
buffer built on these three operations, and `main.ls` uses it to build a
string whose length nothing knew in advance. It is about sixty lines, and
it needed nothing this document did not add.

---

## 5. What this does not add

* **No `realloc`.** §4. It would also be a lie about cost: `realloc` can
  move or extend, and a language whose whole claim is that costs are
  visible should not hide which one happened.
* **No `res` elements.** §2.1.
* **No slicing a slice.** `s[a..b]` is a second referent into one
  allocation, which needs a decision about who owns what and is a
  different feature.
* **No `Vec` in a standard library.** There is no standard library. §4's
  example is a library, and it lives with its program.

---

## 6. Open

| Question | Why it waits |
|---|---|
| Sub-slicing | Two referents, one allocation; an ownership question of its own |
| `realloc` | Hides which of two very different things happened |
| A standard library | Wants a decision about where a library *ships* from, not just where it lives |
| Boxing other unsized things | There are no other unsized things yet |

---

## 7. The must-reject suite

| Fixture | Rule | § |
|---|---|---|
| `boxed_slice_of_res.ls` | Ending a boxed slice runs nothing, so elements are `val` | 2.1 |
| `boxed_slice_leaked.ls` | A boxed slice is `res`; the heap still cannot leak | 3 |
| `unbox_a_boxed_slice_wrongly.ls` | `unbox` hands back a value; a slice has none to hand | 3 |

And the accepting counterparts:

| Fixture | Shows |
|---|---|
| `boxed_slice.ls` | Allocate, write through it, read it back, end it |
| A growable buffer | `examples/buffer/`, as §4 describes |

Plus a conformance test: a negative count traps, and so does a count whose
byte size overflows.
