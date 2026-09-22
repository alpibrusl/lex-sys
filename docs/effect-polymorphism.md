# Effect polymorphism: answered, and the answer is that there is nothing to be polymorphic over

> **Status: a documented no, and the shortest one yet.**
>
> `ROADMAP.md`'s last row says *"a function generic over the row it
> performs. Named nowhere yet, and much larger than anything above"*, and
> `mode-polymorphism.md` §8 repeats it word for word. Those two lines are
> the entire design, which made it the only language row with no document
> behind it.
>
> Counted by reading, the way `line-reading.md` §1 counted its askers:
> **537 functions in this repository write a row, and not one of them
> could be polymorphic in it.** Not "does not need to be" — *could not*,
> because a row here is fixed at the declaration and nothing a function is
> generic over can change which callee it reaches.
>
> Effect polymorphism is a feature about higher-order code. This language
> refuses function values by rule (§2), so the thing it would abstract
> over does not exist.

---

## 1. What the corpus writes

Every `fn` in `std/`, `examples/` and `tests/accept/` with a written row:

| | |
|---|---:|
| functions with a written row | **537** |
| of which declare `[]` | **298** |
| distinct non-empty rows | **19** |
| generic functions carrying a non-empty row | **239** |

And the nineteen are not spread out:

```
 146  [io_write]
  37  [heap]
  19  [heap, io_write]
  15  [ffi("libc")]
   3  [io_read, io_write]
   2  [err_write]
   …
```

Four rows account for **217 of the 239** non-empty ones — 91%. A feature
that lets a function be generic over its row would, in this repository,
be generic over a set of four things that every caller already knows.

That alone is not an argument: `slicing.md` was worth building on one
program's need. §2 is the argument.

---

## 2. A row cannot vary, and that is by construction

`Signature.effects` in `crates/lex-sys-ir/src/lib.rs` carries its own
reason:

> *The declared effect row (§7.2): written at the boundary, never
> inferred across one. A caller is checked against this and never against
> the body that justifies it.*

and a caller reads it straight off the declaration —
`Resolved::Fn(index) => f.signatures[index].effects.clone()`. Not off an
instantiation. So for a row to vary, something a function is generic over
would have to change which callee its body reaches. The two things a
lex-sys function is generic over are:

* **type parameters**, which monomorphisation substitutes into types.
  They do not select a name: a generic body calls the functions it names
  and no others.
* **region parameters**, which are lifetimes and reach nothing at all.

There is no third. No traits, no dispatch on a type, no function values —
`README.md`'s non-goals rule out the first two by name, and the third is
a rule with a fixture:

```
`f` is a function; M1 has no function values, so it can only be called
```

(`Rule::NoFunctionValues`, `tests/reject/function_as_value.ls`.)

So there is no expression in this language whose effect row depends on
anything. A feature for writing down that dependence has nothing to write
down.

---

## 3. The nearest thing to an asker, and why it is not one

`std.io` has this pair:

```
pub fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io_write] int {
    return write_bytes(io, s);
}

pub fn error_all[&r, &i](io: &!i Io, s: &r [byte]) -> [err_write] int {
    return write_err(io, s);
}
```

Two functions, same parameters, same body shape, **different rows**. If
anything in this repository wanted to be row-polymorphic it is this.

It does not, and the reason is the useful part: they differ by the
*builtin they call* as well as by the row. `write_bytes` and `write_err`
are two functions, so collapsing these two wrappers needs a way to be
generic over **which function to call** — a function value — and the row
would then follow from it for free. Row polymorphism is the shadow of
higher-order code, not a feature that stands without it.

The same is true of every other near-duplicate: a pair that differs in
its row differs in its callee first.

---

## 4. What would create an asker

Not a prediction — a list of the things that would have to arrive first,
so the next reader can check rather than re-derive.

| | and then |
|---|---|
| **Function values** | The whole motivation. Excluded by `Rule::NoFunctionValues` and by `README.md`'s non-goals, which rule out the trait-system machinery that usually carries them |
| **A capability interface** | "Write to any sink" — the shape `file-handles.md` §6 would create if a `file_write` landed beside `write_bytes` and `write_err`. That is polymorphism over a *capability*, and the row follows; it is a different feature with a different name, and §3 is the evidence it is the one actually wanted |
| **A third stream** | Two rows over one capability was enough for `standard-error.md` to add a label rather than a capability. A fourth or fifth might change the arithmetic; two did not |

Until one of those exists, the row stays what `linearity-and-effects.md`
§7.2 made it: **written at the boundary, exact, and constant.**

---

## 5. What this does not say

* **Not that the feature is wrong.** In a language with closures it is
  how `map` avoids being written once per effect. The claim here is local
  and checkable: in *this* language, at 537 functions, there is nothing
  for it to quantify over.
* **Not that the row system is finished.** `reach.md` §5 is a real gap —
  `ffi("libc")` is every authority at once, and 15 functions here carry
  it. That is about how *coarse* a label can be, which no amount of
  polymorphism over rows would sharpen.
* **Not a claim about inference.** A body's effects are already computed
  and checked against the declaration; what is fixed is the *declaration*,
  deliberately, so that a caller never depends on a callee's body.
