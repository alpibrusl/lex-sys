# Linearity and effects

> **Status: settled, implementation in progress.** This is the gating artifact
> for M2 ([#2](https://github.com/alpibrusl/lex-sys/issues/2)), and it was
> reviewed and accepted before any M2 code was written. The rules below are
> now the specification the implementation is measured against; §12 lists what
> is still open, and nothing there blocks the rest.
>
> **Implemented: §3 through §8** — modes, linearity, both borrow modes,
> arenas, exact effect rows, narrowing, capabilities and FFI. §2's claim now
> holds in code: an effect *is* a borrowed capability, `[io]` on a signature
> means the function was handed an `&!i Io` it did not create, and the only
> checker that runs over any of it is the linearity and borrow checker §3–§5
> already needed.
>
> §7.4 and §8.4 arrived together, because `Ffi(library)` is the first
> capability that *carries data* and so the first thing there is to narrow.
> A foreign call is now a real call into libc, gated by a capability that
> names the library it reaches.
>
> §6 was the last of them, and it is where §5's bet is paid for: lexical
> regions are cheap to check precisely because a reference cannot outlive its
> block, and an arena is what data that outlives its *creator* lives in
> instead. Both open a region, both are checked by the same occurs-check.
>
> **Not implemented: §9** — the escape hatches.
>
> Where the concrete syntax below differs from what was implemented, §5.3
> says so and why. The syntax here was always illustrative (§1); the rules
> are what this document is for.

This document is the spec the M2 conformance suite encodes. Section 11 is that
suite, written out as a fixture list, and it is the part to argue with hardest:
design churn on this page is cheap, and churn after the checker exists ripples
through the checker *and* every fixture.

---

## 1. How to read this

The syntax below extends M0's. It is illustrative — a concrete syntax is not
settled and the formatter's rules are still open (#1) — but it is used
consistently, so the examples can be read as programs rather than sketches.

Every rule in this document comes with at least one **accept** example and at
least one **must-reject** example. A rule with no must-reject example is not a
rule; it is a hope.

Three words are used precisely throughout:

- **consume** — to use a resource in the one way that ends its life.
- **borrow** — to obtain a non-owning reference, valid for a lexically
  delimited region and no longer.
- **capability** — a value that *is* the authority to do something. Not a
  ticket that is checked, not a permission that is looked up: the authority
  itself, which you either hold or do not.

---

## 2. The one rule

The thesis of lex-sys is that ownership and effects are the same idea, and that
a language designed from scratch can have one system where Rust and Koka have
two. Here is that claim made precise:

> **Prior art, credited late (#75).** **Austral** had already shown the first
> half: capabilities as linear values, with a root capability handed to the
> entry point, checked by the same machinery as every other linear value. What
> this document adds is the second half — that same authority stated as an
> exact effect row — and [`related-work.md`](related-work.md) §2 says what that
> buys and what Austral got to first.

> **The resource rule.** Every resource is created once, used exactly as its
> mode permits, and destroyed once. A function's signature names every resource
> that crosses its boundary: **moved** (by value), **borrowed** (by reference,
> for a named region), or **performed** (as an effect). There is one judgment,
> and the checker makes it once.

The unification is not a slogan about two checkers agreeing. It is an
elaboration:

> **An effect is a borrowed capability, and an effect row is surface syntax for
> the capability parameters a function requires.** `[io]` on a signature means
> the function takes an `&!io Io` it did not create. Elaboration inserts the
> parameter; after elaboration the only checker that runs is the linearity and
> borrow checker. There is no second pass that reasons about effects.

Everything else in this document follows from that sentence. In particular:

| Lex-level claim (#1) | What it actually is after elaboration |
|---|---|
| Allocation is an effect | `alloc` takes `&!h Heap`; a function that allocates borrows a heap capability |
| A heap value is a linear resource | `Box[h] T` has mode `res`; `free` is the function that consumes it |
| FFI is a capability you must be granted | `extern` calls take `&!f Ffi`; there is no ambient way to obtain one |
| Effects are declared, not discovered | The row is part of the signature because the parameters are |
| `[]` means pure | No capability parameters, so nothing outside the function can be observed to change |

The last row is what buys **examples-as-tests**, carried over from Lex: a
function whose row is `[]` can be *run by the checker*, because a pure function
has nothing to run against. That is not a bonus feature bolted on later; it is
the reason the row must be exact.

---

## 3. Modes: `val` and `res`

Every type has a **mode**, fixed at its declaration.

- **`val`** — unrestricted. Copyable, discardable, no obligations. `int`,
  `bool`, `&r T`, and any struct or ADT built only from `val` fields.
- **`res`** — linear. Exactly one use, no implicit copy, no implicit discard.
  Declared with `res`, or inferred for any aggregate containing a `res` field.

```
struct Point { x: int, y: int }          // val: all fields are val
res struct File { fd: int }              // res: declared
struct Pair { a: int, f: File }          // res: inferred, because File is res
```

Mode is structural and computed in one pass over a type's fields. It is never
inferred from use, and there is no mode polymorphism in M2 (see §10).

**Reject — a `res` value copied:**

```
fn twice(f: File) -> [] (File, File) {
    return (f, f);          // ✗ `f` is used twice
}
```

**Reject — a `val` type declared to contain a `res` one:**

```
val struct Wrapper { f: File }   // ✗ a val type may not contain a res field
```

An explicit `val` on a type whose members are all `val` asserts exactly what
absence already checks, so the two are the same declaration and hash the same
(`docs/canonical-ast.md` §3). `res` is different: it is a fact about the type
that every caller can observe, so it changes the hash.

### 3.1 Generics carry mode; type parameters do not

Mode is computed *after* substitution, so a generic type takes its mode from
its arguments: `Held[File]` is `res` and `Held[int]` is `val`, and neither
needed a word written on it. Nothing new is required for that — it falls out
of mode being structural.

A type *parameter*, though, is `val`, because mode is never inferred from use
(§3) and a rigid `T` has no members to read. That has a consequence worth
stating plainly, because §12 lists mode polymorphism as open and
monomorphisation has quietly answered half of it:

> A generic function body is checked once with its parameters rigid, where
> `T` is `val`, and then again for each copy, where `T` is whatever that copy
> instantiated it at. So `fn id[T](x: T) -> T` does work for both modes —
> and `fn sink[T](x: T) -> int { return 0; }` is accepted where it is
> written and refused at the instantiation that leaks.

That is the price, and it is the price §12 predicted: the error lands on the
definition's line, from a call site elsewhere. The message names the
instantiation so it can be found, and `tests/reject/res_leaked_from_generic.ls`
pins the behaviour. What is *not* available is a signature that says "this
works for every mode" and is checked once — that remains open, and is the part
that interacts with every other rule here.

---

## 4. Linearity: linear, not affine

**Decision: linear.** A `res` value must be consumed exactly once on every path.
A value that reaches the end of its scope unconsumed is a compile error.

Two rejected alternatives, and why:

- **Affine** (use *at most* once, silent drop allowed) permits exactly the
  failure the system exists to prevent: a file that is never closed, a
  capability that is never returned, an arena handle that is silently
  abandoned. Affine is linear with the interesting case removed.
- **Linear with implicit destructors**, Rust's answer, reintroduces invisible
  effects. A destructor is code — it can write, it can close, it can fail —
  and running it at a scope exit nobody wrote means the effect row on the
  enclosing function is a lie. "C's effects must not be invisible" (#1) applies
  to our own drop glue first.

So: **no implicit drop, ever.** The obligation is discharged by naming the
consumer.

### 4.1 What consumes a value

Exactly four things, and nothing else:

1. **Passing it by value** to a parameter that takes ownership.
2. **Returning it.**
3. **Destructuring it** in a `let` or a `match` arm. The whole is consumed and
   the parts are produced, each subject to the rule in turn.
4. **Storing it into a `res` aggregate**, which is case 1 by another name —
   the constructor is a function that takes ownership.

There is no `drop(x)` built into the language. A resource is destroyed by the
function that knows how: `close(f)` for a `File`, `free(b)` for a `Box`,
`release(c)` for a capability. Each such function is either a primitive or ends
in destructuring to `val` parts.

Four places would otherwise be a fifth consumer by accident, and each is
refused instead. They are consequences of the list above rather than new rules,
but each has a fixture, because a rule with no must-reject fixture is a hope:

| Written | Why it is refused | Fixture |
|---|---|---|
| `open(1);` as a statement | the value is produced and dropped | `res_discarded.ls` |
| `f.fd` on a `res` `f` | a part read without taking the whole apart is a *borrow*, which is §5 | `res_field_read.ls` |
| `_ =>` on a `res` scrutinee | consumes the value and produces no parts | `res_matched_by_wildcard.ls` |
| `Slot::Full(_)` on a `res` payload | the same, one level down | `res_payload_ignored.ls` |

Assignment is the fifth: `f = open(2)` overwrites whatever `f` held, so a live
`res` binding must be spent before it can be reassigned
(`assign_over_live_res.ls`). A spent one may be, and is live again after.

Destructuring takes the *whole* value apart — every field named, exactly once.
A pattern that names some of the fields would be a partial move, which is not
on the list (`destructure_partial.ls`).

**Accept:**

```
fn read_and_close(f: File) -> [fs] Bytes {
    let (bytes, f) = read_all(f);   // read_all gives the File back
    close(f);                        // consumed exactly once
    return bytes;
}
```

**Reject — unconsumed at scope end:**

```
fn leak(f: File) -> [] int {
    return 0;                        // ✗ `f` is still live
}
```

**Reject — unconsumed on one path only:**

```
fn sometimes(f: File, flag: bool) -> [fs] int {
    if flag {
        close(f);
        return 0;
    } else {
        return 1;                    // ✗ `f` is live on this path
    }
}
```

**Reject — use after move:**

```
fn after_move(f: File) -> [fs] int {
    close(f);
    return size(f);                  // ✗ `f` was consumed by `close`
}
```

### 4.2 Conditional consumption

Every path must agree about what is live. The checker joins branch states at a
merge point and rejects any disagreement, rather than inserting the fix-up Rust
calls drop flags — a dynamic drop flag is a runtime cost hidden behind a static
guarantee, and hidden costs are what a systems language is judged on.

**Reject — branches disagree:**

```
fn disagree(f: File, flag: bool) -> [fs] int {
    if flag { close(f); }            // ✗ `f` is dead here, live in the else
    return 0;
}
```

**Accept — both paths agree:**

```
fn agree(f: File, flag: bool) -> [fs] int {
    if flag { close(f); } else { close(f); }
    return 0;
}
```

Yes, that is verbose. `defer close(f);` is the obvious sugar and it is
deliberately *not* in M2: sugar that expands to a consumption on every exit path
can be added later without changing one rule in this document, and adding it now
would mean debugging the expansion and the checker at the same time.

> **Built — `docs/defer.md`.** And the prediction held: not one rule in
> this document changed. `defer` is expanded during lowering into the
> statement it stands for, once per exit path, so the checker replays
> exactly the events it would have replayed for the version above.

### 4.3 Loops

A loop body must leave the live set exactly as it found it. A body that
consumes a variable declared outside it is rejected, because the second
iteration would use it again.

**Reject:**

```
fn loop_consume(f: File, n: int) -> [fs] int {
    var i = 0;
    while i < n {
        close(f);                    // ✗ consumed inside a loop body
        i = i + 1;
    }
    return 0;
}
```

The check is a comparison of two live sets at the back edge. It is not a
fixpoint: the body is walked once, and the sets must match.

---

## 5. Borrowing: non-owning reads exist, and they are lexical

**Decision: yes, M2 has non-owning references — and no, there is no borrow
checker.** The difference is that a reference's validity is a *lexical* fact
here, not an inferred one.

The mechanism is a region variable introduced by a block:

```
borrow f as &r in {
    let n = size(r);                 // r : &r File
    print_int(io, n)
}
// f is owned again here; r does not exist
```

Four rules, all local (the spelling differs a little from what was built —
see §5.3):

1. **`borrow x as &r in { .. }`** freezes `x` for the block and binds a
   reference of type `&r T`. Frozen means: not movable, not consumable, not
   uniquely borrowable.
2. **`borrow mut x as &!r in { .. }`** locks `x` for the block and binds
   `&!r T`, a unique reference. Locked means: nothing else may touch `x` at
   all — no read, no second borrow, no move.
3. **`&r T` and `&!r T` have mode `val`.** They are copyable and discardable,
   which is sound precisely because the referent is frozen or locked for the
   whole region and the region is a block.
4. **Escape is an occurs-check.** The type of a `borrow` block's result may not
   mention `r`. That is the entire escape rule, it is one traversal of one
   type, and it cannot diverge.

This is Cyclone's idea with Rust's inference deleted. What is gone:

- no non-lexical lifetimes — a region is a block, full stop;
- no lifetime inference — regions are written, or they are function parameters;
- no variance, no subtyping of general types;
- no borrow-checker dataflow — a variable's state is `Owned | Frozen | Locked`,
  a three-valued flag set at block entry and restored at block exit.

**Accept — a reference used and discarded inside its region:**

```
fn describe(f: File, io: &!i Io) -> [io] int {
    borrow f as &r in {
        print_int(io, size(r))
    };
    close(f);
    return 0;
}
```

**Reject — a reference escaping its region:**

```
fn escape(f: File) -> [] &r File {   // ✗ `r` is not a region in scope here
    return borrow f as &r in { r };  // ✗ the block's result type mentions `r`
}
```

**Reject — moving a frozen value:**

```
fn move_frozen(f: File) -> [fs] int {
    borrow f as &r in {
        close(f);                    // ✗ `f` is frozen by the enclosing borrow
        0
    };
    return 0;
}
```

**Reject — two unique borrows at once:**

```
fn double_unique(buf: Bytes) -> [] int {
    borrow mut buf as &!a in {
        borrow mut buf as &!b in {   // ✗ `buf` is locked
            0
        }
    }
}
```

**Accept — two shared borrows at once:**

```
fn double_shared(buf: Bytes) -> [] int {
    borrow buf as &a in {
        borrow buf as &b in {
            return len(a) + len(b);
        }
    }
}
```

### 5.1 Region parameters on functions

A function that takes a reference is region-polymorphic, with the region
written. The binder wears an `&`, so which parameters are regions is a fact
about the declaration rather than something read off the parameter list:

```
fn len[&r](s: &r Bytes) -> [] int
```

At a call site the region parameter is instantiated with the caller's region.
That instantiation is first-order unification of a single name — not constraint
solving, and not something that can fail to terminate.

### 5.2 The outlives relation, and why it is a stack

Two references obtained from different `borrow` blocks have different regions
and cannot be used where one region is expected. Rust solves this with
subtyping and variance. M2 solves it by noticing that regions are introduced by
*nested blocks*, and nesting is a stack:

> `r_inner <= r_outer` holds exactly when `r_outer`'s block lexically encloses
> `r_inner`'s. A value of type `&r_outer T` may be used where `&r_inner T` is
> expected. Nothing else coerces, and `T` never changes.

A function may declare the same relation between its region parameters, and the
call site discharges it with the same lexical lookup:

```
fn copy_into[&dst, &src where src <= dst](d: &!dst Bytes, s: &src Bytes) -> [] int
```

Checking `src <= dst` is a walk up a stack whose depth is the block nesting of
the function being checked. It is O(depth), it has no fixpoint, and the
relation is total because a stack is a total order.

**Reject — unrelated regions:**

```
fn unrelated(a: Bytes, b: Bytes) -> [] int {
    borrow a as &x in { borrow b as &y in { same_region(x, y) } }
    // ✗ `x` and `y` are siblings; neither outlives the other
}
```

Instantiation is one assignment, which has a consequence worth stating: a
function with *one* region parameter requires all its reference arguments to
be at one region, and whichever argument is checked first decides which. Two
references from different blocks cannot both fit, even when one outlives the
other — the coercion runs from the argument to the parameter, not between two
arguments. Declaring two parameters and the `where` clause that relates them
is how a function says it accepts both, and that is exactly what
`copy_into` above is for. Widening this is constraint solving, which §10
spends its whole argument refusing.

### 5.3 What was built, where it differs

§1 says the syntax here is illustrative. Three places where the implementation
chose differently, and why:

- **`borrow` is a statement, not an expression.** lex-sys blocks are
  statement lists with no tail expression, so `borrow f as &r in { r }` has
  nowhere to put a result. It is shaped like `if` and `while` instead, and a
  value leaves a block the way it always has — by `return`, which is the case
  rule 4 is about.
- **A region binder wears its `&`:** `fn len[&r]`, not `fn len[r]`. Reading
  which bracket entries are regions off the parameter list leaves a region
  nobody used ambiguous, and makes every diagnostic about the declaration a
  riddle.
- **Escape is checked in two places, not one.** Rule 4 says the block's
  result may not mention `r`. With no block result, the two ways out are a
  `return` and a binding declared outside the block whose type inference
  fills in from inside it. Both are the same occurs-check over one type;
  `tests/reject/reference_escapes_via_inference.ls` is the second one, and it
  is not hypothetical.

**A place is narrower than it looks.** Writing through a `&!r` needs
somewhere to write *to*, and the left side of an assignment is a whole
binding or a field reached through a unique reference — not a field of an
owned local. `c.n = 2` on a local is refused: that is a partial write, and
what a partial write means for a binding holding a `res` field is a question
§4 does not answer. Assigning the whole value says the same thing and asks
nothing new. The restriction is not load-bearing and can be lifted once §4
has an answer.

There is no `*r` yet: a reference to a struct is read with `.field`, which
covers what §5's own examples do. A reference to a scalar can be made and
passed and not otherwise read, which is a wart rather than a rule.

**A unique reference may be used where a shared one is expected**, and never
the reverse. `&!r T` is `&r T` plus permission to write, so passing one
read-only hands the callee strictly less than it already held. This arrived
with §6 rather than here: arenas hand back `&!a` and nothing else, so
without the coercion no function written against `&r` could touch allocated
data at all. The referent stays invariant either way — "`T` never changes".

**Locking is what makes the implementation cheap.** A reference is a pointer
at a buffer the referent is spilled into for the block. A shared borrow needs
no write-back, because the referent is frozen and the two cannot drift. A
unique borrow reads the buffer back when the block closes — sound precisely
because *nothing else may touch the value while it is locked*, so the buffer
is the only version that moved. Two copies of one `&!r` are two copies of one
pointer, so writes through them alias correctly rather than racing to be last
writer.

---

## 6. Arenas are regions

A region variable is introduced by `borrow`, and by one other thing:

```
region a {
    let node = alloc[a](Node { value: 1 });   // node : &!a Node
    walk(node)
}   // the whole arena is released here, in O(1)
```

`region a { .. }` opens an arena. `alloc[a]` allocates in it and hands back a
unique reference carrying the region. At block exit the arena is released
wholesale — one pointer reset, no traversal, no per-object bookkeeping.

**What escapes: nothing whose type mentions `a`.** This is the same
occurs-check as §5, which is the point — an arena's lifetime and a borrow's
lifetime are one mechanism, not two that happen to look alike.

**Nesting** is the same stack: an inner `region` may hold references into an
outer one (`r_outer <= r_inner` by §5.2), and never the reverse.

**Reject — a reference escaping an arena:**

```
fn escape_arena() -> [] &a Node {    // ✗ `a` is not a region in scope here
    return region a { alloc[a](Node { value: 1 }) };   // ✗ result mentions `a`
}
```

**Reject — an inner region's reference stored into an outer one:**

```
fn outlive(outer: &!o Slot) -> [] int {
    region i {
        let n = alloc[i](Node { value: 1 });
        store(outer, n);         // ✗ `&!i Node` does not outlive `o`
    }
    return 0;
}
```

### 6.1 Arenas hold `val` data only

An arena releases memory. It does not close files, release capabilities or
run anything. So:

> **`alloc[a]` requires its argument's type to have mode `val`.** A `res` value
> placed in an arena would have its memory reclaimed at block exit with its
> linear obligation undischarged — a leak with a static blessing.

**Reject:**

```
fn arena_res(f: File) -> [] int {
    region a {
        let p = alloc[a](f);     // ✗ File is res; arenas hold val data only
        0
    }
}
```

This is restrictive and knowingly so. The alternative — arena teardown as a
consumption event, with per-object finalisers — reintroduces implicit
destructors and turns an O(1) release into a traversal. If it turns out to bite
in practice, the fix is a *separate* `scope` construct with explicit
registration, not a weakening of this rule.

> **What was built.** `region a { .. }` and `alloc[a](v)`, spelled as above.
> The region name is written bare in both, because `&` is the reference
> constructor and there is nothing in either position for it to construct.
>
> **An arena is a `borrow` block with the referent taken out.** Opening one
> pushes a block onto the same table §5 uses, with the same parent link — so
> §5.2's outlives relation, §5's occurs-check and the scope rules apply to
> it without a line of new reasoning. `reference_escapes_arena.ls` is
> refused by the code that refuses `reference_escapes_borrow.ls`, and
> `inner_region_stored_in_outer.ls` by the code behind `unrelated_regions.ls`.
> That is §6's claim — "an arena's lifetime and a borrow's lifetime are one
> mechanism, not two that happen to look alike" — and it is structural
> rather than asserted.
>
> **`alloc[a]` hands back `&!a T`**, an ordinary unique reference. There is
> no second notion of a pointer-into-an-arena, so reading, writing through
> it and passing it to a region-polymorphic function all work unchanged.
> `a` must be an arena open right now: a `borrow` block's region is a region
> but has no chunk behind it, and a region *parameter* is a caller's, not
> this function's to allocate in.
>
> **One new coercion, which §6 forces.** A unique reference is accepted
> where a shared one is expected, never the reverse — `&!r T` is `&r T`
> plus permission to write, so handing one over read-only gives the callee
> strictly less than it already had. Without it nothing written against
> `&r` could touch arena data at all, since `alloc` hands back `&!a` and
> nothing else. The referent stays invariant.
>
> **At runtime** an arena is one `malloc` when the block opens and one
> `free` when it closes, with a bump pointer in between; a `return` out of a
> region releases every arena it leaves, innermost first. Release is one
> call whatever was allocated, which is the O(1) the section is trading
> expressiveness for — and it is *only* a `free`, because §6.1 kept
> everything with an obligation out of the chunk.
>
> **Exhausting the chunk traps.** A single chunk is what makes release one
> call; growing it would make release a walk. The alternative to trapping is
> writing past the end of an allocation, which is undefined behaviour, which
> this language does not have. So the arena is 64 KiB and asking for more
> kills the process, deterministically, the same way division by zero does.
> Growth that keeps both properties is an M3 question and the trap is what
> keeps the answer honest until it is answered.

---

## 7. Effects

### 7.1 Sets, not rows

**Decision: an effect row is a canonically ordered set of labels.** No duplicate
labels, no row variables in M2.

"Rows or sets" (#2) is a question about what duplicate labels buy. They buy
effect handlers and masking — a Koka `catch` that handles the innermost
`exn` and leaves an outer one alone. M2 has no handlers, so duplicates would be
a cost with no purchase. Sets also have the property the rest of this project
needs: a canonical order makes an effect row **hashable**, and a signature that
hashes deterministically is what per-unit identity is made of (#1).

Two operations are needed, and only two:

- **union**, to compute a body's effects;
- **subset**, to check a call against a declaration.

Both are linear in the number of labels, which is small and statically bounded.

### 7.2 Declared at boundaries, inferred inside

**Decision: every function signature declares its row. Inference happens only
within a body.**

Whole-program effect inference is exactly the non-local, potentially-diverging
analysis the totality commitment forbids, and an inferred row is a contract
nobody wrote and everybody depends on. Inside a body there is nothing to infer
but a union over the calls, which is a fold.

The check at a call site is: *the callee's row is a subset of the enclosing
function's declared row*.

**Reject — an undeclared effect:**

```
fn quiet(io: &!i Io) -> [] int {
    print_int(io, 1);                // ✗ `print_int` performs `io`; row is []
    return 0;
}
```

**Every signature writes its row, including `[]`.** An absent row would be an
inferred one, and "inferred at the boundary" is the thing this section
refuses — so purity is written down and visible rather than worked out.

The grounding runs the other way from what a reader might expect: a label is
not *declared* anywhere, it is **performed** by a primitive, and a label
nothing performs can never appear in an exact row. So `[telepathy]` is
refused by §7.3 without anyone maintaining a list of legal effect names, and
adding a new effect means adding something that performs it.

### 7.3 A declared effect that is not performed is an error

Not a warning. The row is exact or it is decoration, and an inexact row means
`[]` no longer means pure — which costs examples-as-tests, and costs a
signature hash that means something.

**Reject — an over-wide row:**

```
fn pure_after_all(io: &!i Io) -> [io] int {
    return 1;                        // ✗ declared `io`, performs nothing
}
```

This is the same rule as `lex agent-guidelines` §1.2 ("narrow effects, always;
if the checker rejects, narrow the *body*, not the signature") turned from a
guideline into a type error. The cost is that a signature cannot reserve room
for a future implementation; that tension is recorded in §12.

### 7.4 Narrowing

An effect label may carry a value, which is what makes `[fs_write("/tmp/x")]`
different from `[fs_write]`. The value comes from the capability:

```
fn write_log(fs: &!f Fs) -> [fs_write(f)] int
```

and `f`'s own type records what it was narrowed to. Narrowing is a function on
capabilities:

```
let logs = narrow_path(fs, "/var/log/app");   // Fs("/var") -> Fs("/var/log/app")
```

**Narrowing only, in both directions.** A capability can be attenuated and
never widened — the same commitment `lex-os` makes for manifests, for the same
reason: a program must not be able to grant itself what it was not given.

In M2 the argument to a narrowing function must be a literal, so the refinement
is checkable structurally. General compile-time refinement waits for
`comptime`, which is deliberately excluded from "minimal" (#1).

**Reject — widening:**

```
fn widen(fs: &!f Fs) -> [fs_write("/")] int {
    return write(fs, "/etc/passwd");  // ✗ `f` is narrowed to "/var/log"
}
```

> **What was built.** `narrow(cap, "libc")`, on the one capability that
> carries a value: `Ffi`. What a capability was narrowed to travels in its
> type — `Ffi("libc")` and `Ffi("libm")` are different types — and the label
> carries the same text, so `ffi` and `ffi("libc")` are different labels and
> a row containing one is a different `SigId` from a row containing the
> other.
>
> **Refinement is prefix extension**, which is what makes it checkable by
> reading rather than by solving: `narrow(c, t)` is accepted exactly when
> `t` extends what `c` already names, and refused otherwise.
>
> *(M3 added `Fs(prefix)` as the second capability carrying a value, over
> the same `narrow`. One rule had to be made sharper for it: a **path**
> prefix extends at a `/` or not at all, because `/tmp` is a byte prefix of
> `/tmpevil` and does not contain it — `fs_sibling_prefix.ls`. A library
> name has no such structure, so `Ffi` keeps the plain textual rule. See
> `docs/filesystem.md` §1.1.)* The unnarrowed
> root is the empty string, a prefix of everything, so the `Ffi` that
> `split` hands out can still become any library while an `Ffi("libcrypto")`
> can never become `Ffi("libc")` — `effect_widened.ls`. Narrowing to what a
> capability already names is refused too: it grants nothing and would read
> as though it had.
>
> **Narrowing consumes.** The wider capability is spent, which is the whole
> point — after narrowing there is no way back to it, because linearity says
> there is no second use. For the same reason a *borrowed* capability cannot
> be narrowed: a borrow is the promise to give it back.
>
> **Covering, not equality.** §8.2's discharge rule reads the same prefix
> order: owning `Ffi("")` discharges every `ffi(...)` label, because its
> holder can narrow to any of them, and owning a `World` discharges both
> `io` and the `Ffi` root. That is why `main` still declares `[]` while the
> program calls into libc.

---

## 8. Capabilities

### 8.1 What one looks like

> **What was built.** `World`, `Io`, `Ffi(library)` and — from M3 —
> `Fs(prefix)`, plus the `Split` that `split` hands back:
> `let Split { io, ffi, fs } = split(world);`. `Heap` still waits for
> general allocation: a capability for an effect nothing can perform is
> decoration, which is what §7.3 refuses for rows and what this refuses for
> the same reason.
>
> A capability the program does not need is still a resource, so a program
> that calls into no library releases its `Ffi` rather than ignoring it.
> Adding a capability to `Split` is therefore a breaking change every
> program has to acknowledge — which happened twice, for `Ffi` and again
> for `Fs`, each time touching every program in the tree. That is the
> honest cost of there being no ambient authority, and it is the cost being
> paid rather than avoided.
>
> The prelude's types are predeclared rather than written in a program, and
> a program may not declare its own — `capability_redeclared.ls`. Nor write
> one as a literal: `Io { }` would be an ambient constructor spelled
> differently, so it is refused (`no_ambient_capability.ls`). Since `World`
> and `Io` carry no fields, taking one apart would end authority without
> naming a consumer, so that is refused too and `release` is the only way
> (`capability_destructured.ls`). `Split` is destructured on purpose.
>
> **Owning discharges; borrowing declares.** §8.2's remark that `main`'s row
> is `[]` is implemented as a rule: an effect whose capability a function
> owns *by value* does not appear in its row, because ownership is already
> visible in the parameter list and is strictly stronger than borrowing. A
> borrowed `&!i Io` discharges nothing — it is exactly what `[io]` names.


A capability is an ordinary `res` value. It has no special kind, no special
syntax, and no runtime representation beyond what its type says — most are
zero-sized, and the ones that are not (a file descriptor, an arena pointer) are
the size of the thing they authorise. **Capabilities erase at compile time
except where they carry data.** Threading one costs nothing.

```
res World                     // the root of all authority
res Heap                      // allocation
res Io                        // console
res Fs(prefix: Path)          // filesystem below a prefix
res Ffi(library: Name)        // foreign calls into one library
```

### 8.2 Where one comes from

One place:

```
fn main(world: World) -> [] int {
    let Split { heap, io, fs, ffi } = split(world);   // consumes `world`
    ...
}
```

`World` is linear and the runtime hands over exactly one. `split` consumes it.
There is no ambient constructor, no `Io::global()`, no `unsafe { }` that
conjures one. **A function that is not given a capability cannot perform its
effect**, which is the whole safety story, stated as a type.

That `main`'s own row is `[]` is not a quirk: a row lists the capabilities a
function *borrows*, and ownership is already visible in the parameter list.
Owning authority is strictly stronger than borrowing it, and strictly more
visible.

### 8.3 How one is threaded

By borrow, not by move — a callee should not consume its caller's authority:

```
fn greet(io: &!i Io) -> [io] int {
    return write_str(io, "hello\n");
}

fn main(world: World) -> [] int {
    let Split { heap, io, fs, ffi } = split(world);
    borrow mut io as &!i in { greet(i) };
    release(heap); release(io); release(fs); release(ffi);
    return 0;
}
```

The four `release` calls are the linearity rule doing its job: authority is a
resource, and a resource is destroyed exactly once. A program that forgets one
does not compile.

**Reject — an effect performed without the capability:**

```
fn sneaky() -> [io] int {
    return write_str(global_io(), "hello\n");   // ✗ no such thing as global_io
}
```

**Reject — a capability used after release:**

```
fn after_release(io: Io) -> [io] int {
    release(io);
    borrow mut io as &!i in { greet(i) }        // ✗ `io` was consumed
}
```

### 8.4 FFI

A foreign call is an effect like any other, and its capability names the
library:

```
extern fn strlen(f: &!ffi Ffi, s: &r Bytes) -> [ffi("libc")] int
```

The declaration is the only place a foreign signature is written, the
capability is the only way to reach it, and the row makes the call visible in
every caller's signature all the way up. C's effects stop being invisible at
the exact point they enter the program.

> **What was built.** `extern fn` declarations, lowered to real imports and
> called through the platform's own convention:
>
> ```
> extern fn labs[&f](ffi: &f Ffi("libc"), n: int) -> [ffi("libc")] int;
> ```
>
> A foreign declaration is a signature with no body, so its `SigId` and
> `BodyId` are the same hash — there is nothing else it could be a hash of.
> It is region-polymorphic like any other function, because the capability
> it takes is borrowed, but it takes no type parameters: C has none to
> instantiate.
>
> **The declaration is where the rules are enforced**, because it is the
> only place the signature is written. Three of them:
>
> * the row and the capability parameters must agree exactly, in both
>   directions — a declaration naming an effect it holds no capability for
>   is `ffi_without_capability.ls`, and one holding a capability it does not
>   declare is `foreign_effect_undeclared.ls`;
> * the borrowed capability must name a library. The unnarrowed root names
>   none, so `Ffi("")` is refused here — narrow first;
> * only what C can name crosses: `int`, `bool`, `()`, and borrowed
>   capabilities, which do not cross at all. An aggregate has no layout
>   agreement between the two sides yet, and a reference to anything but a
>   capability would be a pointer this compiler has not promised to lay out.
>
> **`int` at the boundary is the platform's 64-bit integer**, not C's
> `int` — which is why the fixtures call `labs` rather than `abs`. A
> narrower C integer type needs a type to name it, and M2 does not have one.
>
> **The capability does not travel.** It is checked, then erased: what libc
> receives is the integer and nothing else. `narrowed_capability.ls` calls
> `labs(-7)` and prints `7`, which it could not do if a zero-sized
> capability were pushed in front of the argument.
>
> A foreign name is not also a written function's name: two answers to one
> call is one answer too many.

---

## 9. Escape hatches

> **Built, and this section was half wrong. See `docs/sharing.md`.**
>
> Both hatches below were written. `Gen` is a library exactly as described
> and now lives in `examples/slab/`. `Rc` is **not expressible at all**:
> it needs a copyable pointer, and this language has none — `Box[T]` is
> `res`, a reference is bounded by its region, and there is no third
> thing. Three ways of trying are three fixtures in `tests/reject/`, each
> refused by a different rule that was not written with `Rc` in mind.
>
> So "neither touches the checker" is the claim that did not survive
> being built: a real `Rc` is a language feature, not a library.
> `sharing.md` §2.3 has what you can have instead — shared ownership is
> reachable, the *ambient handle* is not.
>
> The rest of this section stands, and `sharing.md` supersedes the `Rc`
> row of the table.

Linearity cannot express shared mutable graphs, observers, or any structure
where "who owns this" has no answer. Two hatches, both **libraries, not
language features** — neither touches the checker, and that is the point:

| Hatch | What it is | Cost, paid where |
|---|---|---|
| `Rc[h] T` | Non-atomic reference count, allocated from `h` | Clone: one increment. Release: one decrement and a branch. Dereference: free. **Cycles leak** — there is no collector and there will not be one |
| `Gen[T]` | A generational handle `{index, generation}` into a `Slab[T]` | Dereference: a bounds check, a generation compare, and a branch. A stale handle **fails**, returning `None` — it is never undefined |

Both are `res`. `Rc` is consumed by `release_rc`, a `Slab` by `free_slab`.
Neither is reached for by default, and neither hides its cost: the runtime cost
table above is part of the contract, in the same spirit as `[budget]`.

`Gen` is the one to prefer. A stale generational handle is a *value* — `None` —
and the program decides what to do about it. A dangling pointer is undefined
behaviour and the program does not get a say. That asymmetry is most of why
this language exists.

That last paragraph turned out to be stronger than it knew: `Gen` is not
merely the one to prefer, it is the only one of the two there is. See
`docs/sharing.md`.

---

## 10. Why the checker stays fast and total

The commitment is "fast and total" (#1), which means: every check terminates,
and the cost is bounded by the size of the function being checked. What earns
that here:

1. **No global inference.** Every signature is complete — types, regions,
   effects. A function is checked against its callees' signatures, never
   against their bodies.
2. **No fixpoints.** The loop rule compares two live sets at a back edge rather
   than iterating to convergence. Branch joins compare and refuse rather than
   patching up.
3. **Regions are a stack.** `<=` is a lexical lookup, not a constraint graph.
4. **Effects are sets.** Union and subset over a small, statically bounded
   label set.
5. **No subtyping** except the one region coercion, which never changes the
   type it decorates. No variance. No trait resolution — there are no traits
   (#1, non-goals).
6. **No higher-order functions in M2**, so no effect row variables and no mode
   polymorphism. This is the single largest simplification in this document,
   and it is why the row-versus-set question in §7.1 could be answered so
   cheaply. When closures arrive they bring row variables with them, and this
   section is where the argument has to be re-made.

Each function's check is one walk of its body carrying a three-valued state per
local and a set per call. That is linear in body size with an O(nesting-depth)
lookup, and nothing in it can diverge.

---

## 11. The must-reject suite

This is the deliverable the gate actually cares about: the rules above,
restated as fixtures. M0 already has the harness — `tests/reject/*.ls`, each
fixture declaring its own expected message in a `//~ ERROR` header, run by
`crates/lex-sys/tests/conformance.rs`. M2 adds to it; it does not invent
anything.

A ✓ in the last column means the fixture exists and the rule is enforced.
Every row carries one.

| Fixture | Rule | § | |
|---|---|---|---|
| `res_copied.ls` | A `res` value may not be used twice | 3 | ✓ |
| `val_contains_res.ls` | A `val` type may not contain a `res` field | 3 | ✓ |
| `unconsumed_at_scope_end.ls` | A live `res` value at scope end is an error | 4 | ✓ |
| `unconsumed_on_one_path.ls` | Every path must consume | 4 | ✓ |
| `use_after_move.ls` | A consumed value may not be used | 4.1 | ✓ |
| `branches_disagree.ls` | Branches must agree about what is live | 4.2 | ✓ |
| `consume_in_loop.ls` | A loop body may not consume an outer binding | 4.3 | ✓ |
| `reference_escapes_borrow.ls` | A block's result may not mention its region | 5 | ✓ |
| `move_while_frozen.ls` | A frozen value may not be moved or consumed | 5 | ✓ |
| `read_while_locked.ls` | A uniquely borrowed value may not be read | 5 | ✓ |
| `two_unique_borrows.ls` | One unique borrow at a time | 5 | ✓ |
| `unrelated_regions.ls` | Sibling regions do not outlive each other | 5.2 | ✓ |
| `region_param_unsatisfied.ls` | A declared `<=` must hold at the call site | 5.2 | ✓ |
| `reference_escapes_arena.ls` | Nothing mentioning the arena's region escapes it | 6 | ✓ |
| `inner_region_stored_in_outer.ls` | An inner region's reference may not be stored outward | 6 | ✓ |
| `arena_holds_res.ls` | `alloc` takes `val` data only | 6.1 | ✓ |
| `undeclared_effect.ls` | A call's row must be a subset of the declared row | 7.2 | ✓ |
| `effect_declared_not_performed.ls` | An over-wide row is an error | 7.3 | ✓ |
| `effect_widened.ls` | A capability may be narrowed, never widened | 7.4 | ✓ |
| `no_ambient_capability.ls` | There is no way to obtain a capability but to be given one | 8.2 | ✓ |
| `capability_used_after_release.ls` | A capability is a resource | 8.3 | ✓ |
| `capability_leaked.ls` | A capability must be released | 8.3 | ✓ |
| `ffi_without_capability.ls` | A foreign call requires its `Ffi` capability | 8.4 | ✓ |

§7 adds three more: `effect_not_propagated.ls` (a row is transitive),
`ungrounded_effect_label.ls` (a label nothing performs) and
`effect_row_required.ls` (the syntax).

§8.4 adds one beyond the table — `foreign_effect_undeclared.ls`, a foreign
declaration holding a capability its row does not name.

§8 adds three: `world_leaked.ls` (the root is a resource too),
`capability_destructured.ls` (taking one apart is not releasing it) and
`capability_redeclared.ls` (a program may not declare its own `Io`).
`main_takes_the_world.ls` replaces M0's `main_takes_arguments.ls`, since
`main` now takes exactly one thing.

§5 adds eight must-reject fixtures beyond the table, for the rules §5.3
describes and for the syntax: `region_not_in_scope.ls`,
`reference_escapes_via_inference.ls`, `assign_while_frozen.ls`,
`borrow_after_move.ls`, `borrow_mode_mismatch.ls`,
`unique_borrow_of_frozen.ls`, `write_through_shared.ls` and
`assign_field_of_local.ls`. Its accepting counterparts are
`borrow_and_return.ls`, `two_shared_borrows.ls`, `nested_regions.ls` and
`unique_borrow.ls`.

§4.1's four accidental consumers and §3.1's instantiation rule add six more
must-reject fixtures beyond the table — `res_discarded.ls`,
`res_field_read.ls`, `res_matched_by_wildcard.ls`, `res_payload_ignored.ls`,
`assign_over_live_res.ls`, `destructure_partial.ls` and
`res_leaked_from_generic.ls` — plus `var_destructure.ls` for the syntax. Their
accepting counterpart is `destructuring.ls`.

Each fixture is the smallest program that triggers its rule and nothing else.
An accepting counterpart goes in `tests/accept/` for every one of them — a rule
that rejects everything is not a rule either:

| Fixture | Shows | |
|---|---|---|
| `consume_once.ls` | The straight-line happy path | ✓ |
| `consume_on_both_paths.ls` | Branch agreement | ✓ |
| `borrow_and_return.ls` | A borrow used and discarded inside its region | ✓ |
| `two_shared_borrows.ls` | Shared borrows nest | ✓ |
| `nested_regions.ls` | An inner region reading an outer one | ✓ |
| `arena_roundtrip.ls` | Allocate, walk, release in O(1) | ✓ |
| `effect_exact.ls` | A row that is exactly what the body performs | ✓ |
| `narrowed_capability.ls` | Attenuation, and a call that fits inside it | ✓ |
| `threaded_io.ls` | `main` splitting `World` and threading `Io` down three frames | ✓ |

---

## 12. Open questions

These are genuinely open. They are *not* blocking — each can be settled after
M2 code exists without invalidating a rule above — but they are the places to
push if something here feels wrong.

- **Reserving room in a row.** §7.3 makes an unperformed declared effect an
  error, which forbids a signature that anticipates its implementation. Is that
  right for a published library, where widening a row later is a breaking
  change? An explicit opt-out is easy to add and hard to take back.
- **`defer`.** *Answered — see `docs/defer.md`.* Yes, still visible, and
  the question turned on what the word means here: not "written on the
  line where it runs" but **the type says what happened**. The effect is
  still in the row, the exactly-once rule is still enforced on every
  path, and the line is still written — one below the acquisition, which
  is where a reader looks for the pairing. A destructor would have failed
  that test; `defer` names the function.
- **Capability release at `main`.** *Answered `no` — see
  `docs/authority.md`.* The affine-hole argument holds, and it is not the
  interesting one: even if reclaiming were safe it would be wrong, because
  those four lines are the only place a program's authority surface is
  written down. `main` owns rather than borrows, so its row is `[]` and
  every entry point in this repository has the same signature — the
  releases are the declaration, not ceremony around it. What the tooling
  owed was to *read* them, which `lex-sys authority` now does.
- **Mode polymorphism.** *Answered — see `docs/mode-polymorphism.md`.* The
  half-answer below was right: monomorphisation does make `fn id[T](x: T) -> T`
  work at both modes. The signature that says so is `[T: val]`, checked once
  and enforced at the call site, and an unbounded parameter is checked as
  `res`. Checking the claim also found a **leak and a double free**: a `val`
  on a *generic* declaration was trusted rather than checked, so
  `Wrap[Box[int]]` was `val` by assertion. The original text follows.

  *Half-answered by §3.1:* monomorphisation does make
  `fn id[T](x: T) -> T` work at both modes, because each copy is checked at
  the type it was instantiated at. What is still open is a signature that says
  so and is checked *once* — the part that interacts with every rule here at
  once, and the part that would keep the error off the definition's line.
- **`Rc` cycles.** Documented as a leak. A systems language may be entitled to
  say exactly that, or a `Weak` may be table stakes.
- **Budget.** *Answered `no` — see `docs/budget.md`.* It does not belong
  here. `lex-os` already separates the three questions a capability
  language faces — *may this reach X at all* (the type check, which is
  this document), *which host or path* (the perimeter), and *how much*
  (the supervisor, charged per mediated command) — and a budget's units
  settle it: wall-clock seconds, commands, **money in cents**. None of
  those is a property of a program's text. What was wanted is
  legibility rather than enforcement, and that is
  `lex-sys authority --output json`.

---

## 13. What this forces on M1

M2 cannot be bolted onto an M1 that made these impossible. Three things the
type checker must get right the first time, before any of the above is written:

1. **Signatures are complete and are the unit of checking.** A body is checked
   against signatures only. If M1 lets any part of a signature be inferred from
   a body, §10.1 is already lost.
2. **Types are values the compiler can compare cheaply and hash canonically** —
   including, eventually, regions and effect rows. An effect set's canonical
   order is not an afterthought; it is what makes a signature hash.
3. **The branch join is a real operation.** M1's checker should join branch
   states even when the only state is types, because §4.2 is that same join
   with a live set added.

Nothing in M1 needs to know what a capability is. It does need to be built so
that adding one is adding a rule, not rewriting a pass.
