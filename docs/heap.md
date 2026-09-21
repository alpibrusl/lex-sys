# The heap

> **Status: settled, and built in the same change.** The gate for the last
> unchecked box on M2's list ([#1](https://github.com/alpibrusl/lex-sys/issues/1)):
> *"escape hatches with documented runtime cost"*. §9 of
> `linearity-and-effects.md` named two — `Rc` and `Gen` — and both are
> libraries over an allocator that did not exist. This document is that
> allocator.
>
> It is also the answer to the one thing the README has said was missing
> since M2: **a general heap**.

---

## 1. What is actually missing

Everything in this language so far lives in a frame or an arena. Both are
*lexical*: a value's lifetime is a block, and §5's escape check exists to
make sure nothing outlives the block it was written in.

That is a real memory discipline and it covers most programs. What it cannot
express is a value whose lifetime is **decided by the program rather than by
the source text** — a node added to a structure that outlives the function
that made it, a result handed back up, anything recursive.

The concrete symptom, today:

```
struct Node { value: int, next: Node }
// error: type `Node` contains itself, so it has no finite size
//        (M1 has no references)
```

Every linked structure in computing is that declaration plus one indirection.
This document adds the indirection.

---

## 2. `Heap` is the fifth capability

`linearity-and-effects.md` §8.1 listed it first and built it last, for the
reason §7.3 gives: a capability for an effect nothing can perform is
decoration.

```
res Heap                      // allocation
```

It carries no value, like `Io` and unlike `Ffi(library)` and `Fs(prefix)` —
there is nothing to narrow, because a heap has no parts to name. It
discharges one plain label, `heap`, and a function that allocates says so:

```
fn make[&x](h: &!x Heap, n: int) -> [heap] Box[int]
```

It is borrowed **uniquely**, like `Io` and unlike `Ffi` and `Fs`. Those two
are keys: holding one changes nothing, and two holders at once is the same
as one. An allocator has *state*, and the honest type for shared mutable
state is the one that says only one reference reaches it.

`split` therefore hands back four capabilities:

```
let Split { io, ffi, fs, heap } = split(world);
```

which breaks every program in the tree for the third time. §8.1 already
calls that the honest cost of there being no ambient authority, and it is
still the right trade: a program that does not allocate releases its `Heap`
and says so in one line.

---

## 3. `Box[T]` — one value, one allocation

```
res Box[T]
```

Three operations, and they are the whole surface:

```
box(h: &!x Heap, value: T) -> [heap] Box[T]
unbox(h: &!x Heap, b: Box[T]) -> [heap] T
contents(b: &r Box[T]) -> [] &r T        // and &!r Box[T] -> &!r T
```

`box` allocates `size_of(T)` bytes and moves the value in. `unbox` consumes
the box, frees the allocation, and yields the value back. `contents` is the
dereference: it borrows what the box owns, for exactly as long as the box
itself is borrowed, and preserves the borrow's mode.

```
borrow b as &r in { total = contents(r).x + contents(r).y; }   // read
borrow mut b as &!w in { contents(w).x = 10; }                 // write
```

### 3.0 What `contents` cannot do, and why that is not the heap's fault

This language has **no dereference operator**. A reference is read through
field access (`h.fd`) or indexing (`s[n]`), and there is no syntax for
reading a whole *scalar* through one — `&r int` has been unreadable since
M2, and nothing in this document changes that.

So `contents` is what you want for a box of a struct, an enum or a slice,
and for a box of an `int` it hands back a reference with nothing to do. Read
a boxed scalar with `unbox`, which is the operation that was going to end
the box anyway.

That is a real sharp edge and it is stated rather than papered over. It is
also **not a heap problem**: the missing thing is a general dereference, it
was missing before this document, and giving `Box` a private one would hide
a gap the language should close in the open. §7 lists it.

**`Box[T]` is `res` whatever `T` is**, including `Box[int]`. `T`'s mode says
whether the *contents* must be consumed; the box is `res` because it owns an
allocation, and that is true of a box of anything.

### 3.1 The heap cannot leak

This is the property worth the whole section.

A `Box[T]` is a linear resource. §4's rule is that a `res` value is consumed
exactly once on every path — so a box that is never unboxed is a **compile
error**, at the point the program forgot it, with the same message an
unreleased capability gets.

*(That this holds at all depends on `reading-references.md` §2.0, which was
added later. Until it was, a `res` field could be read out of a shared
reference — including a `Box` — which produced a second owner and a real
double free. The claim in this section was false for as long as that hole
was open, and is true with it closed.)*

So in this language, *the general heap does not leak*. Not "should not":
cannot, checked, before the program runs. Nor can it double-free or
use-after-free, for the same reason from the other direction — `unbox`
consumes, and using a consumed value is refused.

That is a stronger claim than either escape hatch in §9 will be able to
make. `Rc` leaks cycles by construction, and says so. A `Box` does not,
because there is nothing for a cycle to be made of: a box is owned by
exactly one place, always, and the compiler knows which.

The checker guaranteeing `unbox` runs is a claim about the *program*, so
the claim about the emitted code is checked separately, two ways:

* under valgrind on linux-x86_64 — one million allocs, one million frees,
  *"in use at exit: 0 bytes in 0 blocks"*, no errors;
* in CI on both targets, by `the_heap_actually_frees`: eight million 2 KiB
  boxes one at a time, which is one box's footprint if freeing works and
  16 GB if it does not. A regression that dropped the `free` does not
  produce a worse number there, it fails outright.

### 3.2 What it costs

In the spirit of `[budget]` and §9's cost table, stated rather than hidden:

| Operation | Cost |
|---|---|
| `box(h, v)` | One `malloc`, plus storing `v` |
| `unbox(h, b)` | One `free`, plus loading `v` |
| `contents(b)` | One load. Not a check — there is nothing to check |

No header, no refcount, no tag: `Box[T]` at run time is a pointer and
nothing else, which is why `contents` is a load and why a box costs the same
as the `malloc` it is. `malloc` returning NULL traps, the way an exhausted
arena does (§6) — this language does not have undefined behaviour to
continue into.

---

## 4. Recursive types, through a box and only through a box

The size check stays exactly as it was, with one hole in it:

> **A type may contain itself if every path back to itself passes through a
> `Box`.**

Because `Box[T]` is a pointer, it is one leaf however large `T` is, so the
size computation terminates. The refusal in §1 is unchanged for a type that
contains itself *directly* — that still has no finite size, and the error
still says so.

```
enum Tree {
    Leaf,
    Node(Box[Tree], int, Box[Tree]),
}
```

This is the payoff. It is also, in a linear language, more interesting than
it looks:

```
fn drop_tree[&x](h: &!x Heap, t: Tree) -> [heap] int {
    match t {
        Tree::Leaf => { return 0; }
        Tree::Node(left, value, right) => {
            let l = unbox(h, left);
            let r = unbox(h, right);
            return drop_tree(h, l) + drop_tree(h, r) + 1;
        }
    }
}
```

Every node is freed exactly once, and **a version of this function that
forgot a subtree would not compile** — `left` and `right` are `res` values
produced by the match, and §4 requires both. The traversal that frees a tree
and the proof that it freed all of it are the same code.

### 4.1 Reading a recursive structure means consuming it

The second sharp edge, found the same way as §3.0's — by writing the
fixture.

**`match` requires ownership.** A borrowed enum cannot be matched
(`&l List` is refused with "cannot be matched"), and that has been true
since M1; nothing about the heap changes it. So there is no way to walk a
linked list *without* taking it apart, and a read-only traversal of a
recursive structure is not expressible today.

For a box of a **struct** this does not bite, because field access already
reaches through a reference: `contents(r).x` reads without consuming, and
`tests/accept/box_roundtrip.ls` does exactly that. It bites for enums, which
is what every recursive type needs a variant of.

The consequence is worth stating plainly rather than hiding: in this
language today, **walking a list frees it**. That is a real restriction and
also a strangely honest one — the traversal and the proof it released
everything are one piece of code, which is the property §3.1 is about. What
is missing is matching through a reference, which needs binding modes and is
a type-system feature of its own. §7 lists it.

---

## 5. Heap or arena?

Both exist on purpose and neither replaces the other. The choice is not
about performance first; it is about whether a lifetime is lexical.

| | Arena (§6) | Heap |
|---|---|---|
| Lifetime | A block | Decided by the program |
| Escapes its scope | No — that is the escape check | **Yes** — that is the point |
| Allocations | One `malloc` for the region | One `malloc` each |
| Release | One `free`, whole region, O(1) | One `free` each, by linearity |
| Forgetting to release | Impossible — the block ends | Impossible — the box is `res` |
| Recursive types | No | Yes |

The arena is still the right default: bulk lifetime, bulk cost, nothing to
track. Reach for a box when a value has to outlive the block that made it,
or when a type has to contain itself.

---

## 6. What this does not add

* **No `Box[[T]]`.** Boxing an unsized referent means a box carrying a
  length, which is a second shape with its own rules. Slices come from
  arenas (`docs/strings.md` §5) and that stays true.
* **No `Rc`, no `Gen`.** §9's two hatches are *libraries*, and this language
  has no module system to put a library in. They also need the thing this
  document deliberately does not have: a way to share. Both wait, and §9's
  cost table already says what they will cost when they arrive.
* **No realloc, no growable buffer.** `Vec` is a library over a heap and a
  copy, and it waits for the same reason.
* **No custom allocators.** `Heap` is one capability naming one allocator.
  An allocator parameter is `Rc[h] T`'s `h`, which is §9's problem.

---

## 7. Open

| Question | Why it waits |
|---|---|
| `Box` of a slice | A second shape: pointer plus length, and who owns the length |
| Sharing at all | `Rc` and `Gen` (§9). Needs modules first |
| Growable buffers | A library over box + copy; needs `Box[[T]]` |
| Custom allocators | Wants a heap *parameter*, not a heap capability |
| `contents` on a moved-from box | Not expressible — linearity refuses it before it can be asked |
| A dereference operator | §3.0. `&r int` has been unreadable since M2; the fix is a language feature of its own, not a `Box` method |
| Matching through a reference | §4.1. Needs binding modes; until then a recursive structure is read by consuming it |

---

## 8. The must-reject suite

Stated in advance, as every section of `linearity-and-effects.md` §11 was.

| Fixture | Rule | § |
|---|---|---|
| `box_without_heap.ls` | Allocating requires a `Heap` | 2 |
| `box_leaked.ls` | A `Box` that is never unboxed is refused | 3.1 |
| `box_used_after_unbox.ls` | `unbox` consumes; using the box after is refused | 3.1 |
| `heap_effect_undeclared.ls` | A row that allocates must declare `heap` | 2 |
| `recursive_without_box.ls` | A type containing itself *directly* still has no size | 4 |
| `contents_escapes_its_borrow.ls` | `&r T` from a box dies with the borrow | 3 |
| `box_destructured.ls` | A box is ended by `unbox`, not by a pattern | 3 |

And the accepting counterparts:

| Fixture | Shows |
|---|---|
| `box_roundtrip.ls` | Box a value, read it through a borrow, unbox it |
| `linked_list.ls` | A recursive type, built and freed by linearity |

Plus `the_heap_actually_frees` (§3.1), because whether the emitted code
frees is not a property any single program's types can state.

An allocation that fails **traps** rather than returning a null nobody
checked, the way an exhausted arena does (§6). That one has no conformance
test of its own and this document will not claim otherwise: a test would
have to exhaust the machine's memory, where the arena's chunk is a fixed
64 KiB and can be exhausted in a loop. It is the same `trapz` on the same
`malloc` result, and `exhausting_an_arena_traps_rather_than_running_past_the_chunk`
covers that shape.
