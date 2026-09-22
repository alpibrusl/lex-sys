# Slicing

> **Status: settled and built, and the case for it was made by a library
> that already shipped.**
>
> `std.bytes.find` returns an index into a slice, and until now a program
> had **no operation that could take it**. The library handed back a
> position and nothing to do with it, and `starts_with` exists only
> because `equal(text[0..len(prefix)], prefix)` could not be written.
>
> It also turned out to be the precondition for the roadmap's "writer
> abstraction" — see §6, which is the part of this document worth
> reading even if the feature is obvious.

---

## 1. The form

```
s[a..b]        // half-open: a included, b excluded
```

`s` is `&r [T]` or `&!r [T]`; the result is a slice of the same element
type, the same region and the **same mode**. `len(s[a..b])` is `b - a`.

Half-open because `b - a` is the length, `s[a..a]` is empty, and
`s[a..b]` followed by `s[b..c]` is `s[a..c]` with nothing counted twice
or missed. Every off-by-one this language can avoid, it should.

There is no `s[a..]` or `s[..b]` yet (§7): `len(s)` is one call and an
abbreviation is not a design.

## 2. Bounds trap

`0 <= a <= b <= len(s)`, checked at run time, and a violation **traps**.

That is `defined-behaviour.md` §8's rule for indexing, applied to the
operation that produces a range rather than an element — and it has to
be, because the alternative is a slice that claims a length its
allocation does not have, which is a buffer overrun with a type on it.

`a > b` traps rather than yielding empty. An inverted range is a bug in
the program that wrote it, and silently returning nothing is the kind of
defined-but-wrong answer `defined-behaviour.md` §2.1 refuses: an
operation with no right answer stops rather than inventing one.

There is no unchecked form. The check is two comparisons against a
length that is already in the second register of the slice.

## 3. It is an ordinary reference

The result carries `s`'s region, so:

- it cannot escape the block that region belongs to — the same
  occurs-check, over the same type, with nothing added;
- it may be used where `&r [T]` is wanted, by the coercion §6 of
  `linearity-and-effects.md` already has;
- it is `val`, so it copies and discards like any other reference.

None of that needed a rule. A subslice is a reference, and references
already have all of these.

## 4. Why a unique slice stays unique

This is the one decision, and the answer is already in
`linearity-and-effects.md` §5:

> Two copies of one `&!r` are two copies of one **pointer**, so writes
> through them alias correctly rather than racing to be last writer.

So `&!` here does **not** mean what `&mut` means in Rust. It is not a
no-aliasing invariant over references; it is a lock on the *binding* for
the block. (This paragraph was right and `README.md`'s performance
section was wrong; what changing it would cost is measured in
[`aliasing.md`](aliasing.md), and the answer is no.) Within the block the referent is spilled to exactly one
buffer, nothing else may touch the value at all, and every reference
derived from the borrow points into that one buffer. The write-back at
block exit restores the whole of it.

`s[0..3]` and `s[1..4]` overlap. Under Rust's rules that is the thing
`split_at_mut` exists to avoid; here it is the same situation as two
copies of one `&!r`, generalised from "the same address" to "overlapping
addresses", and the argument that made the first sound makes the second
sound. One buffer, one write-back, one locked binding.

The consequence worth stating: **a `split_at` that hands back two
disjoint halves is not a safety feature here.** It would be a
convenience, and §7 leaves it open on those terms rather than as a hole.

## 5. What it costs

A slice is a pointer and a length, in two registers. `s[a..b]` is

```
(ptr + a * stride, b - a)
```

— one multiply-add and one subtract, after two comparisons. Nothing is
copied, nothing is allocated, and there is no descriptor: the same two
values a slice always was.

## 6. And this is the answer to "a writer abstraction"

The roadmap has carried *a writer abstraction* as the next thing for
several slices. Working out what one would be is how this feature was
found, and the conclusion is that **there should not be one.**

A `Writer` would be a value a function takes so it can send bytes
somewhere without knowing where. Written as an enum over the
destinations:

```
enum Writer { Console(..), File(..), Buffer(..) }
```

a function taking one must declare the **union** of what every arm
could do — `[io_write, fs_write(""), heap]` — on every call, including
the ones that only ever touched the console. That is precisely the
inexactness §7.3 refuses: an exact row means *what this function did*,
and a union row means *what it might have done depending on a value the
type cannot see*. A language whose whole claim is that the row is the
truth cannot buy convenience with a row that is not.

So the abstraction is not a writer. It is a **buffer**:

```
b = fmt.int(heap, b, 42);              // [heap], and nothing else
b = fmt.text(heap, b, " items");       // [heap]
borrow b as &r in {
    console.write_all(io, bytes(r));   // [io_write] — exact
    fs_write(f, "/tmp/out", bytes(r)); // [fs_write("/tmp")] — exact
}
```

Formatting touches the heap. Emitting touches the destination. Each row
says exactly what its function did, "where does this go" is a fact about
the call site rather than about a runtime value, and the intermediate
buffer is the price — which is the same price the enum would have paid
anyway, minus the dispatch.

**That is what needed slicing.** A `Buffer` holds more bytes than it
uses, so handing its contents to anything taking a `&r [byte]` means
handing over exactly `used` of them, and there was no operation that
could say so. `std.buffer.bytes` is `contents(b.held)[0..b.used]`, one
line, and the writer question is closed with a document rather than a
feature.

## 7. Open

| Question | Why it waits |
|---|---|
| `s[a..]` and `s[..b]` | `len(s)` is one call; an abbreviation is not a design, and two forms to parse is a cost with nothing behind it |
| `split_at` | §4: not a safety feature here, so it is a library convenience and wants a reason beyond symmetry with languages where it is one |
| Slicing an *owned* run | There is no owned run to slice: a `[T]` is always behind a reference, because it is unsized |
| Patterns over slices | `match` on a length is a different feature (`reading-references.md` §6) |

## 8. The suite

| Fixture | Rule | § |
|---|---|---|
| `slice_of_a_non_slice.ls` | `..` needs a run to range over | 1 |
| `slice_bound_is_not_an_int.ls` | Both bounds are `int` | 1 |
| `slice_escapes_its_region.ls` | A subslice is an ordinary reference, so it cannot outlive the borrow | 3 |

§2's two bounds rules are **runtime** traps, so they are conformance
tests rather than reject fixtures — the reject harness runs `check`, and
a program that traps is one that compiled:

| Test | Rule |
|---|---|
| `slicing_past_the_end_traps` | `b > len(s)`, and a negative bound read as unsigned |
| `an_inverted_range_traps` | `a > b` stops rather than yielding empty |

| Accepting | Shows |
|---|---|
| `slicing.ls` | A subslice read, re-sliced, passed where `&r [T]` is wanted, and emitted to two destinations with an exact row each |
