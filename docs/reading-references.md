# Reading through a reference

> **Status: settled, and built in the same change.** The gate for the two
> limits `heap.md` §3.0 and §4.1 found and did not fix. They looked like two
> problems and are one, which is why they get one document.

---

## 1. One gap, two symptoms

This language can *make* references — `borrow`, `&!a T` from an arena,
`contents` on a box — and it can follow them in exactly two ways: field
access (`h.fd`) and indexing (`s[n]`). That is all.

Two things fall out of the hole that leaves.

**A scalar behind a reference cannot be read.** `&r int` has existed since
M2 and there has never been a way to get the `int` out of one. `heap.md`
§3.0 records it: `contents` on a `Box[int]` hands back a reference with
nothing you can do to it.

**A recursive structure cannot be read without destroying it.** `match`
requires ownership, so taking a list apart to look at it *is* taking it
apart. `heap.md` §4.1 records it, and `examples/tree.ls` is built around
it: the walk that prints the tree is the walk that frees it, because no
other walk is expressible.

The second is the one that matters. A structure you can only read by
consuming is a structure you can read *once*, and "traverse this twice" is
not an exotic requirement — it is `len` then `sum`.

---

## 2. The rule

> **A reference gives references.**

Matching `&r E` binds every payload as a reference into the enum, carrying
the scrutinee's **mode** and its **region**:

```
match list {                       // list: &l List
    List::Empty => { .. }
    List::Cons(value, rest) => { .. }   // value: &l int, rest: &l Box[List]
}
```

Nothing moves out of a reference, ever. That single sentence is the whole
type rule, and it is what keeps linearity intact: a payload that is `res`
binds as a *borrow* of that `res`, so no obligation is created, nothing is
consumed, and the scrutinee is exactly as owned after the match as before
it.

### 2.0 The same rule, for field access

Stated here for `match`, and it holds for the other way to reach into a
value. **Field access is not an exception**, and for two separate
stretches it was one.

The rule decides *what comes back*, not whether anything does:

| Through a reference | `val` member | `res` member |
|---|---|---|
| `match` payload | borrows — `*value` reads it | borrows |
| field access `h.f` | **copies** | **borrows** |

A `val` field is copied out, which costs the referent nothing and is what
a reference is for. A `res` field cannot be copied — that is what `res`
means — so what comes back is a **reference to** it, exactly as `match`
on a reference binds a `res` payload. `h.held` on a `&r Holder` is a
`&r Box[int]`, carrying the base's mode and its region: a field of a
`&!r` is reachable uniquely, a field of a `&r` is not, and either way the
field's reference lives exactly as long as the one it was reached
through.

The remaining difference in that table — a `val` field copies where a
`val` payload borrows — is the ergonomic one, and it is the reason
`b.used` is not `*b.used`.

#### The hole this closed, twice

Reading a `res` field through a reference was first **allowed and
unsound**. It produced a second owner of a value the referent still
owned, and under valgrind that was an `Invalid free()` from ordinary
code with no `unsafe` anywhere — which this language does not have. It
contradicted `heap.md` §3.1's "a double free is unexpressible".

It was then **refused outright**, which fixed the unsoundness and
overshot: a struct with a `res` field could not be read through a
reference *at all*, while the equivalent enum could. Two standard-library
modules paid for that with an accessor that takes the whole value and
hands it back — `std.buffer`'s `write` and `std.vec`'s `get` — and
`collections.md` §5.1 recorded it as the next thing to settle.

A borrow is the answer to both, and the reason is worth stating plainly:
**the double free was never about reading, it was about owning.** So the
refusal belongs at the use rather than at the read, and it is already
there. Every route to a second owner is refused by the ordinary type
rule, because `&r Box[int]` is not `Box[int]`:

```
unbox(heap, holder.held)          // expected `Box[?0]`, found `&r Box[int]`
Holder { held: holder.held }      // expected `Box[int]`, found `&r Box[int]`
return holder.held;               // expected `Box[int]`, found `&r Box[int]`
```

That is one rule doing the work two were doing, and the surviving one is
the rule the language already had.

#### Reaching a `res` field on an *owner* is still refused

Unchanged, and for a different reason: there is no reference involved, so
reading the field would **move** it out of a value that is still whole,
leaving a half-consumed aggregate the checker cannot describe. Take it
apart with a destructuring `let`, which names every part at once.

### 2.1 Why not Rust's binding modes

Rust infers whether a pattern binding is a move, a copy, a `&` or a `&mut`,
from the scrutinee and the pattern together. It is genuinely convenient and
it is genuinely subtle — the rules have been revised more than once, and
"what mode is this binding in" is a question a reader has to reconstruct.

This language does not do implicit. The rule above has no modes to infer
and no defaults to remember: match an owned enum and you get owned
payloads, match a reference and you get references of the same mode. Which
one you are in is visible in the scrutinee, one line up.

### 2.2 Two unique bindings from one match

`match` on `&!r E` binds each payload as `&!r T` — several unique
references at once, into one referent.

That is sound for the same reason two fields of a struct can both be
written: the payload positions of a single variant are **disjoint**. They
are different offsets in the same value, no two patterns name the same
one, and a variant's payload cannot overlap itself. Nothing here needs an
aliasing analysis; it needs the layout, which the backend already has.

---

## 3. `*r` — the dereference

```
*r          // r: &q T  ->  T,  when T is `val`
*r = v      // r: &!q T,  when T is `val`
```

Reading copies the value out. Writing replaces it. Both require `T` to be
**`val`**:

* copying a `res` out of a reference would duplicate an obligation — two
  values, one of which nobody is required to consume. §4 exists to make
  that impossible and this does not get an exception;
* writing over a `res` would drop whatever was there without naming a
  consumer, which is the same silent drop `assign_over_live_res.ls`
  already refuses.

A `res` behind a reference is read the way it always was — by borrowing it
further, or by field access — and ended by whatever function ends it, on
the owner.

`*` is prefix, so it never collides with multiplication: `a * *b` is a
product of `a` and what `b` points at, and the parser knows which position
it is in. It binds tighter than any binary operator and looser than
postfix, so `*p.x` is `*(p.x)` and `(*p).x` is written with the
parentheses — which `p.x` already does for you, since field access reaches
through a reference on its own.

---

## 4. What this unlocks

A read-only traversal of a recursive structure, which is the thing that was
impossible:

```
fn total[&l](list: &l List) -> [] int {
    match list {
        List::Empty => { return 0; }
        List::Cons(value, rest) => { return *value + total(contents(rest)); }
    }
}
```

`value` is `&l int` and `*value` reads it. `rest` is `&l Box[List]`,
`contents` follows the box to `&l List`, and the recursion borrows for
exactly as long as this frame does. The list is not touched: it is still
owned by the caller, still owes exactly one traversal that ends it, and can
be read as many times as anyone likes first.

`examples/tree.ls` stops having to compute everything in the pass that
frees: `contains`, `deepest` and `tally` take `&t Tree`, hold no capability
at all, and leave the tree as owned as they found it. The consuming walk is
still there and still the only thing that ends the tree — it simply is no
longer the only thing that can *look* at one.

`tests/accept/match_a_reference.ls` is the minimal version: a list read
three times, then freed once.

---

## 5. What this does not add

* **No `ref` patterns and no `&` patterns.** There is one rule and the
  scrutinee decides it. A pattern that could override the mode would be
  the inference §2.1 is refusing.
* **No moving out of a reference**, with or without a marker. `res`
  ownership lives with the owner; a reference is a promise to give it back.
* **No nested patterns.** A pattern is still one variant and a name per
  payload position, matching M1. Nesting is orthogonal to this and can come
  later without changing anything here.
* **No `match` on a reference to a struct.** A struct is destructured with
  `let`, and reaching its fields through a reference already works.

---

## 6. Open

| Question | Why it waits |
|---|---|
| Nested patterns | Orthogonal; this rule composes with them unchanged |
| `match` on a slice | Needs patterns over lengths, which is a different feature |
| Moving out of a unique reference | Would need the referent marked as moved-from; a real feature, and not one anything needs yet |
| ~~Reading a `res` field *as a borrow*~~ | **Done, and the reason given here was wrong.** It said this "is exactly the binding-mode question §2.1 declines". It is not: §2.1 declines *inferring* among several possible modes, and for a `res` field there is only one — it cannot be copied, so a borrow is the only thing reading one could mean. Nothing is inferred, and §2.0 hands back `&r Field` |
| `*r` on a `res` behind a *unique* reference | A swap, not a read: it would have to put something back. Wants its own operation |

---

## 7. The must-reject suite

| Fixture | Rule | § |
|---|---|---|
| `deref_a_res.ls` | Copying a `res` out of a reference duplicates an obligation | 3 |
| `deref_a_non_reference.ls` | `*` follows a reference; there has to be one | 3 |
| `write_through_shared_deref.ls` | `*r = v` needs a unique reference | 3 |
| `match_reference_binding_escapes.ls` | A binding from a matched reference dies with the region | 2 |
| `match_reference_payload_consumed.ls` | A `res` payload bound by reference may not be consumed | 2 |
| `res_field_read_through_reference.ls` | A borrowed `res` field may not be **consumed** — refused at the use, by the type | 2.0 |
| `res_tuple_component_through_reference.ls` | The same, where the aggregate is a tuple | 2.0 |

And the accepting counterparts:

| Fixture | Shows |
|---|---|
| `borrowed_fields.ls` | A `res` field and a `res` tuple component read through a reference, at both modes — and `std.buffer` printed **twice** without being spent |
| `match_a_reference.ls` | A list read twice and then freed |
| `deref_roundtrip.ls` | `*r` reads, `*r = v` writes, through the right modes |
