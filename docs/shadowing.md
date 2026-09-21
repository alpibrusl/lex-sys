# Shadowing

> **Status: settled. The last item on `sharing.md` §4's list.**
>
> Three things made `examples/slab/` more verbose than it should have
> been. `docs/tuples.md` closed two. This is the third, and it is the
> one where the restriction turned out to be a real rule stated too
> bluntly rather than an omission.

---

## 1. What asked for it

`sharing.md` §4, third entry:

> **No shadowing within a block.** Threading a value through several
> steps means `fresh`, `live`, `emptied`, `stale` — four names for one
> slab — because `let slab = ...` twice in a block is refused.

`examples/slab/main.ls` still reads that way after the tuple slice:
`fresh`, `filled`, `live`, `emptied`, `checked`, `refilled`, `reused`,
`stale`. Eight names for one slab, none of which is a different thing
from the last.

This is the shape a linear language pushes every program into. A value
that is threaded is consumed and returned, so the returned one needs a
name, so the next step needs another. The verbosity is not incidental to
linearity — it is what linearity *looks like* without shadowing.

---

## 2. Why it was refused, which was not arbitrary

The obvious reading is that the rule was conservatism. It was not.

```
let held = box(h, 1);
let held = box(h, 2);   // and the first allocation?
```

If this were allowed, the first `held` would be unreachable with its
obligation undischarged. Nothing could ever `unbox` it. That is a leak,
and it is precisely the case §4 of `linearity-and-effects.md` says the
system exists to prevent:

> *a value that reaches the end of its scope unconsumed is an error,
> because the case affine drops silently is the one the system exists to
> prevent.*

So the blanket refusal was the correct first answer to a real hazard. It
was simply broader than the hazard.

### 2.1 The language already shadows, where it is safe by construction

`let` in an inner block shadows an outer binding today, and always has.
So does a `let` over a function parameter — the body is a block, so a
parameter is always in an enclosing scope:

```
fn f(n: int) -> [] int {
    let n = n + 1;     // legal, and always was
    return n;
}
```

The rule was never "no shadowing". It was "not within one block", and
the same-block case is the only one where a live obligation could be
stranded — because in every other case the shadowed binding is still
reachable when its own block closes, and the block-close check catches
it there.

---

## 3. The rule

> **Shadowing is allowed exactly when the shadowed binding is dead.**

A binding is dead when it holds no outstanding obligation: either it is
`val`, so it never had one, or it is `res` and has been consumed.

That is not a new rule. It is the rule assignment already has:

```
x = e;          // refused if `x` still holds a live `res` value
let x = e;      // refused if `x` still holds a live `res` value
```

`tests/reject/assign_over_live_res.ls` has existed since M2. One rule,
two syntaxes, and the second one now says so.

### 3.1 `val` shadows freely

Nothing is owed, so there is nothing to strand:

```
let n = read_digit(s, 0);
let n = n * 10 + read_digit(s, 1);
```

### 3.2 `res` shadows after it is consumed

Which is the threading case, and it works for a reason that is not a
special case:

```
let slab = new_slab(heap, 4);
let (slab, handle) = insert(slab, 7);
let slab = remove(slab, handle);
```

A `let`'s initialiser is lowered **before** the binding exists — the
rule that makes `let x = x;` read the outer `x` or fail, never itself.
So by the time the new `slab` is declared, `insert` has already consumed
the old one and it is dead. Shadowing needs nothing added for this; it
falls out of an ordering that was already there for a different reason.

### 3.3 And the leak is still a leak

```
let held = box(h, 1);
let held = box(h, 2);
```

*"`held` still holds a `res` value; shadowing it here would put that
value out of reach with its obligation undischarged. Consume it first."*

---

## 4. Where it is checked, and why not in the parser

At **replay**, not during lowering.

Whether a binding is dead is a fact about the trace, and lowering cannot
know it: a value may be consumed by a call whose argument types are
still inference variables at the moment the call is lowered, and settle
only later. That is the same reason modes are decided at replay rather
than when a type is first written (`linear.rs`, the module comment).

So lowering records the **link** — this declaration shadows that slot —
and the checker reads the liveness. Which is why this could not have
been a one-line change in the parser, and why it is a slice rather than
a patch.

### 4.1 It also moves a diagnostic to where the mistake is

Shadowing a live `res` **parameter** was already caught, because the
value stayed live to the end of the function. But the refusal landed on
the `return`:

```
fn f(t: Ticket) -> [] int {
    let t = 1;
    return t;        // error: `t` is still live here
}
```

`t` on that line is an `int`. The message was true of a binding the line
does not mention. It now lands on the `let`, which is the line that is
wrong.

---

## 5. What this does not change

* **A pattern still binds each name once.** `let Pair { a, a } = p;` and
  `Shape::Rect(w, w)` are still refused: one pattern, one binding per
  name. Shadowing is between statements, never within one.
* **A `borrow` region name is not a binding.** Regions live in their own
  namespace (§5 of `linearity-and-effects.md`), and nothing here touches
  it.
* **A frozen binding cannot be shadowed out from under a reference** —
  but not because of a new check: a `borrow` block is a block, so its
  body is an inner scope, and there is no way to write a `let` in the
  same block as the `borrow` that is also inside it.
* **`var` is still assignment.** `var x = 1; x = 2;` reassigns one
  binding; `let x = 1; let x = 2;` makes two. The difference matters
  when a reference is involved, and both obey the same liveness rule.

---

## 6. Open

| Question | Why it waits |
|---|---|
| A warning for shadowing that is probably a mistake | A lint, not a rule, and this language has no lint pass |
| `_` as a binding that discards a `val` | §5 of `tuples.md`'s open list; still a decision about discards |

---

## 7. The suite

| Fixture | Rule | § |
|---|---|---|
| `shadow_over_live_res.ls` | A shadowed binding must be dead | 3.3 |
| `shadow_over_live_res_destructured.ls` | The same, through a destructuring pattern | 3 |
| `shadow_a_live_parameter.ls` | The same, and now reported at the `let` | 4.1 |
| `binding_repeated_in_pattern.ls` *(exists)* | One pattern, one binding per name | 5 |
| `assign_over_live_res.ls` *(exists)* | The same rule, the other syntax | 3 |

One fixture was **removed**: `rebind_same_block.ls`, which was

```
let x = 1;
let x = 2;
```

and which asserted the old rule directly. It is the program this slice
exists to allow, so it could not be kept — and a must-reject fixture
disappearing is worth naming rather than leaving to a diff. What
replaces it is the three above: the same shape, with a value that is
actually owed something.

| Accepting | Shows |
|---|---|
| `shadowing.ls` | `val` shadowed freely, `res` shadowed after consumption, and an inner block shadowing an outer live binding |
| `examples/slab/` | Rewritten: one `slab` threaded through, rather than eight names for it |
