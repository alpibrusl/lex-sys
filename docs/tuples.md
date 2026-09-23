# Tuples

> **Status: settled, and asked for by a library rather than by a design.**
>
> `docs/sharing.md` §6 calls this "the single biggest ergonomic gap a
> linear language has". That is not a judgement made in the abstract:
> `examples/slab/` is sixty lines, and the lack of tuples shaped three
> of its decisions. This document is the smallest thing that closes the
> gap without adding a second aggregate to the language.

---

## 1. What asked for it

`sharing.md` §4 is a list of three things writing one library surfaced.
The first:

> **No tuples.** `insert` must answer with both a slab and a handle, so it
> returns a `res struct Inserted { slab, handle }` declared for the
> purpose. Every operation that threads the slab needs one.

`examples/slab/slab.ls` declares two types, `Inserted` and `Looked`,
that exist only because a function may return one value. Neither is a
concept in the library. Both have to be named, documented, kept in step
with their function, and read by anyone reading the code.

That is the cost of the missing feature in one small library, and it
scales with how linear the code is: a linear API *threads* its state,
which means handing it back, which means handing back two things.

A second entry on that list turns out to be the same entry:

> **No renaming in a destructuring pattern.** `let Slab { entries, live }`
> binds those names and no others, so two slabs cannot be taken apart in
> one scope.

A struct pattern binds field names because the fields *have* names. A
tuple pattern has nothing to inherit, so it must name its bindings — and
therefore may name them anything. Tuples close two of §4's three gaps,
and the second one for free.

---

## 2. What a tuple is

**An anonymous struct with positional fields.** Not a new kind of
aggregate: everything below is a rule structs already have, restated for
a type with no declaration.

```
fn insert(s: Slab, value: int) -> [] (Slab, Gen) {
    ...
    return (Slab { entries: entries, live: live + added }, Gen { index: at, generation: g });
}

let (slab, handle) = insert(fresh, 7);
```

### 2.1 Two components or more

`(e)` is grouping and stays grouping — parentheses are formatting, and
the parser leaves no node behind for them. A one-tuple would therefore
need a second spelling, `(e,)`, which is a syntax whose only purpose is
to disambiguate something nobody asked for.

There is no `()` either. The checker has a `Unit` type for "this
expression yields nothing", and it is deliberately **not writable in
source**: making it writable is a decision about whether a function may
return nothing, which is a different question from whether it may return
two things. A tuple has two components or more, and that is the whole
rule.

Note what that does and does not refuse. `()` is an error — it parses as
a tuple and is rejected for having no components. `(e,)` is an error the
same way — the comma makes it a tuple, and it is rejected for having one.
`(e)` is **not** an error: it is grouping, and was grouping before tuples
existed.

> **Corrected.** This paragraph used to say there was no fixture for a
> one-tuple "because there is no one-tuple to write". That was false: the
> parser has always accepted a trailing comma, so `(e,)` parsed as a
> one-part tuple, and the checker refused it with the rule above. Nobody
> had written it; the fuzzer did (`docs/fuzzing.md`), and found that the
> printer dropped the comma, turning a refused program into an accepted
> one on reprint. The printer now keeps it, and `one_tuple.ls` pins the
> refusal. The rule — two components or more — did not change.

### 2.2 Structural, so there is no declaration

`(int, Gen)` is the same type wherever it is written. This is the first
**structural** type in the language: every other aggregate is a `DefId`
into a table of declarations, and two declarations with identical fields
are two types.

That matters more here than it would elsewhere, because of
`many-files.md`. A program is the set of files named on the command line
sharing one flat namespace, and until now two files could only agree on
an aggregate if one of them declared it. They can now agree on `(int,
Gen)` with neither declaring anything — the first type in this language
that crosses a file boundary with no declaration site at all.

### 2.3 Mode is computed, not declared

A tuple is `res` if any component is `res`, and `val` otherwise.

Every other aggregate **declares** its mode, and `res struct` / `val
struct` is checked against the members: `docs/sharing.md` §2.1's third
refusal is exactly that check. A tuple has no declaration site, so there
is nowhere to write a mode and nothing to check the members against. The
mode is read off the components instead.

This is not a loosening. The rule a struct is checked against —
*a `val` type may not hold a `res`* — is a rule about declarations, and
a tuple makes no declaration, so the rule is **vacuous** rather than
violated. A tuple holding a `Box[T]` is `res`, and every obligation that
follows from `res` follows here exactly as it does for a struct.

It does mean the mode of a tuple is not written anywhere in the source.
That is the one thing this feature costs in legibility, and it is the
same thing `mode_of` already computes for `Pair[File]` versus
`Pair[int]`, where the mode of a generic type has never been written
either.

---

## 3. What you can do with one

| Written | Means |
|---|---|
| `(a, b)` | Construct. Components evaluate left to right, like every other argument list (`defined-behaviour.md` §3) |
| `let (x, y) = t;` | Destructure. Consumes `t`, binds the components under the names written |
| `t.0`, `t.1` | Positional field access. Exactly `p.x` with a number instead of a name |
| `(A, B)` in a type | A type, usable wherever a type is: a parameter, a return, a struct field, a type argument, a box's referent |

### 3.1 `.0` obeys the rules `.x` already obeys

Three cases, none of them new:

* **On an owner, all components `val`.** Copied out. The tuple is read.
* **On an owner, any component `res`.** Refused: *"a field cannot be read
  out of it; take the whole value apart"*. The same refusal a `res`
  struct gives, for the same reason — reading one component would leave
  the others owed by nobody.
* **Through a reference.** A `val` component is copied; a `res` component
  is refused by `reading-references.md` §2, *nothing moves out of a
  reference, ever*. That rule cost a double free to find last time it was
  not enforced somewhere it applied, so it is enforced here from the
  start rather than after.

### 3.2 A pattern names every component

`let (a, b) = t;` on a three-tuple is refused. Destructuring takes the
**whole** value apart, which is what makes it the operation that
discharges a linear obligation — a pattern that could leave a component
unnamed would be a way to drop one.

There is no `_` in a tuple pattern, because there is none in a struct
pattern. For a `val` component it would be a discard and harmless; for a
`res` one it would be a leak, and a rule that holds for one mode and not
the other is two rules. §6.

---

## 4. What this is not

* **Not multiple return values.** `return (a, b)` builds a *value*. The
  caller may destructure it, pass it on, store it in a struct, or put it
  in a box. A calling convention that returned two things would do none
  of that.
* **Not tuple structs.** `struct Pair(int, int)` is a named type with
  positional fields, which is a third aggregate. A named type that wants
  positional fields can have fields called `first` and `second`.
* **Not nested patterns.** `let ((a, b), c) = t;` is refused — this
  language has no nested patterns anywhere, and tuples are not the place
  to introduce them. Take the outer one apart, then the inner.
* **Not a `match` scrutinee.** A tuple has one shape, so a `match` on one
  would have exactly one arm, which is a destructuring `let` written
  longer.

---

## 5. Layout and cost

A tuple's leaves are its components' leaves in order. That is a struct's
layout with the names removed, so there is no new layout decision, no
tag, and no padding question that was not already answered.

A return of more than two leaves travels through memory rather than
registers, which is a rule that already exists and that `Inserted`
already hit: `(Slab, Gen)` is five leaves — a pointer, a length, and
three `int`s — and returns exactly the way `Inserted` did.

**Replacing a declared struct with a tuple changes no generated code at
all.** That is a claim about a compiler rather than about a design, so it
is asserted rather than argued: `a_tuple_emits_the_same_object_as_the_
struct_it_replaces` compiles two programs differing only in whether the
pair is a `res struct` or a tuple, and checks that the **object files are
byte-identical**. Not similar, not the same size — the same bytes.

That is what makes this an ergonomic feature rather than a
representation choice, and it is why `examples/slab/` could drop two
declared types without anyone having to ask what it cost.

---

## 6. Identity

`canonical-ast.md` hashes a type by what it is. A named type's identity
runs through its `DefId`, which is an index into a table of declarations
in the order the program declared them. A tuple has no `DefId` and needs
none: its identity is its constructor and its components, hashed in
order.

So a tuple type is the first type here whose hash is a function of the
type alone, with nothing in it that depends on what else the program
declared. That is what §2.2's "two files agree without declaring" is,
stated as a hash.

The pattern is the mirror of it. A struct pattern encodes its **field
names**, because which field each binder takes is what the pattern says.
A tuple pattern encodes only its **arity**: the names are the pattern's
own invention (§1), so renaming them changes nothing any caller or any
later reader of the value can observe, and the body hash does not move.
That is the same rule that makes `f[T](x: T)` and `f[U](x: U)` one
signature, applied one level down.

---

## 7. Open

| Question | Why it waits |
|---|---|
| Shadowing within a block | `sharing.md` §4's third gap, and the one tuples do not close. Still open, still small |
| `_` in a pattern, struct or tuple | §3.2. Worth having; a decision about discards, not about tuples |
| Nested patterns | §4. A pattern language is its own document |
| One-tuples and a writable unit type | §2.1. Both are decisions about *nothing*, which is harder than it sounds |
| Tuple structs | §4. A third aggregate needs to earn its place |

---

## 8. The suite

Stated before the code, the way `linearity-and-effects.md` §11 was.

| Fixture | Rule | § |
|---|---|---|
| `tuple_arity_mismatch.ls` | A pattern names every component | 3.2 |
| `tuple_index_past_the_end.ls` | `t.2` on a two-tuple is not a field | 3.1 |
| `res_tuple_component_read.ls` | A `res` tuple is taken apart, not read | 3.1 |
| `res_tuple_component_through_reference.ls` | Nothing moves out of a reference | 3.1 |
| `tuple_leaked.ls` | A `res` tuple is consumed exactly once | 2.3 |
| `empty_tuple_type.ls` | A tuple has two components or more | 2.1 |
| `one_tuple.ls` | A tuple has two components or more, `(e,)` included | 2.1 |
| `tuple_type_mismatch.ls` | `(int, bool)` and `(bool, int)` are different types | 2.2 |
| `nested_tuple_pattern.ls` | No nested patterns | 4 |

| Accepting | Shows |
|---|---|
| `tuple_roundtrip.ls` | Construct, return, destructure, and `.0` on a `val` tuple |
| `tuple_of_res.ls` | A `res` tuple threaded through a function and ended |
| `examples/slab/` | Rewritten: `Inserted` and `Looked` deleted, and the generated code unchanged |
