# Compile-time data

> **Status: settled and built.**
>
> `compile-time.md` §8 called this *"the one that pays"* and pointed at
> `examples/base64/`, which scans 64 entries per decoded character where
> GNU uses a 256-entry table.
>
> **§8 was right about the table and wrong about what needed building.**
> The table is worth **5.7×** end to end, measured (§1) — and it was
> already writable, in 36 lines, without any language change. So this
> document has a smaller job than §8 gave it, and §1 is the correction
> before §2 is the design.

---

## 1. The correction: the table was already writable

`examples/base64/` decoding 5.4 MB, best of five:

| | time | against coreutils |
|---|---|---|
| The scan, as shipped | 293 ms | 21× slower |
| A 256-entry table, **built at run time in a `region`** | **51 ms** | 3.6× slower |
| GNU coreutils `base64 -d` | 14 ms | — |

So the scan was costing 5.7×, and the fix was a `region` in `main`, a
`build_table` loop, and a `&t [int]` parameter on the two functions
between them — a 36-line diff, all of it expressible before this slice.
Both binaries agree with coreutils byte for byte on the same input.

**That is the number, and compile-time data does not deliver it.** A
reader of §8 would have expected this document to report 5.7×; the 5.7×
belongs to the table, and the table belongs to anyone who writes it.

### 1.1 So what is left for the feature

Four things, and they are worth stating precisely because they are
smaller than §8 implied:

1. **The plumbing.** 36 lines and a region parameter threaded through
   every function between the table and its use. In base64 that is two
   signatures; in a program where the table is used five levels down it
   is five.
2. **A table an arena cannot hold.** An arena is one 64 KiB chunk and
   exhausting it traps (`defined-behaviour.md` §4). A 65 536-entry
   `[int]` table — the shape a CRC or a 16-bit codec uses — is 512 KB
   and **traps**, measured. So it needs `Heap`, and a program that
   released `heap` cannot have one at all. `examples/base64/` and
   `examples/newton.ls` both release `heap` in their first three lines.
3. **Read-only pages.** A table in the binary is mapped from the file
   and shared; a table built at startup is dirtied anonymous memory,
   per process.
4. **Tables that cannot be written as literals.** `strings.md` §4
   refuses `\x` escapes, so a 256-byte table with arbitrary values is
   not expressible as a string literal either. Without this feature the
   loop is not a preference, it is the only option.

Number 2 is the one that is a *capability* argument rather than a
convenience, and it is why this was built rather than closed as "write
the loop".

---

## 2. The `static` item

```
static decode_table: [int] {
    let table = alloc_slice[static](256, 0 - 1);
    let alpha = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    var i = 0;
    while i < len(alpha) {
        table[int_of(alpha[i])] = i;
        i = i + 1;
    }
    return table;
}
```

A `static` item is **a function body with no parameters, no effect row
and no run time**. It is evaluated once, during compilation, and its
result becomes read-only data in the binary. Elsewhere the name is a
`&static [int]`, exactly like a string literal:

```
fn value_of(c: int) -> [] int {
    return decode_table[c];          // one index, bounds-checked as ever
}
```

Written as a body rather than as an expression because that is the shape
the language already has: `let`, `var`, `while`, `return` and the
linearity checker all apply unchanged, and there was no block-expression
form to borrow. A `static` is a `fn` that cannot be called and does not
need to be.

**No effect row is written and none may be.** A `static` performs
nothing by construction — every effect needs a capability, a capability
is a parameter, and this has no parameters. The row is `[]` and saying so
would be decoration (`linearity-and-effects.md` §7.3).

### 2.1 `alloc_slice[static]` and where it is legal

`static` has always been a region: it is what a string literal lives in,
it outlives everything, and `strings.md` §4 reserves it as a binder
nobody may declare. This slice adds the one way to put something new
there:

```
alloc_slice[static](count, fill)       // and alloc[static](value)
```

**Legal only inside a `static` item's body**, and refused anywhere else.
The rule is lexical rather than a reachability analysis, for the reason
§4.1 of `compile-time.md` gives about diagnostics: a rule a reader can
check by looking at one function beats a rule that depends on what the
whole program reaches.

The escape check needs nothing new. It already answers "does this
outlive that?" with `true` for `Region::Static` against everything
(`strings.md` §4 predicted exactly this: *"the escape check needs no
change at all"*), so a slice allocated in `static` may be returned,
stored, and read from anywhere — which is the whole point.

`&!static` stays refused. There is no unique reference into read-only
data, which was already the rule for literals and is now the rule for
one more reason: the data is in the binary's read-only pages and writing
through it would fault.

---

## 3. Failure is a compile error, not a fallback

`compile-time.md` §5 made a point of the opposite: running out of fuel
while folding a call is **never** an error, because the call can simply
be emitted and run instead.

**A `static` has no such fallback.** Nothing runs at program start,
there is nowhere to put a runtime initialiser, and `alloc_slice[static]`
has no runtime meaning at all. So when a `static` cannot be evaluated —
the fuel runs out, or its body reaches something the evaluator does not
implement — the program is **refused**, and the diagnostic says which.

That is `lex-os-resolver`'s rule in a compiler: *refuse, don't
downgrade.* The alternative — quietly computing the table at startup —
would mean a program's authority and its startup cost depended on how
clever the evaluator was feeling, which is exactly the invisible
dependency §5 was protecting against.

It also means the fuel budget **is** observable here, unlike in §5. A
`static` that needs more than the budget does not compile. That is a
real cost and the honest mitigation is that the budget for a `static` is
larger than the one for a folded call: a table is built once and read for
the life of the program, so it is worth more compile time than an
expression that merely saves a few instructions.

---

## 4. What the evaluator had to learn

`compile-time.md`'s evaluator declined everything to do with memory —
§3.1 said so, and a `static` is nothing but memory. So it gained one
thing: a **store**, holding the slices allocated in the static region.

| | |
|---|---|
| `alloc_slice[static](n, fill)` | Appends `n` copies of `fill` to the store; the value is a slice handle |
| `s[i]` | Reads, with the same bounds check the runtime does — and out of range is a **compile error**, because a certain trap is one (`compile-time.md` §4) |
| `s[i] = v` | Writes through the handle |
| `len(s)` | The length |
| `s[a..b]` | A handle into the same run |

Elements may be `int`, `bool`, `byte` or `float`. Structs, enums and
nested slices are **not** supported — §6 — and a `static` holding one is
refused rather than half-evaluated.

Everything else about the evaluator is unchanged, and that is the part
worth noting: a `static` body may call any pure function, and those calls
go through the same `Machine` that folds `factorial(5)`. The table is
built by ordinary lex-sys code.

---

## 5. What it is worth, and what it is not

`examples/base64/` rewritten on a `static` table, same 5.4 MB of input,
same method as §1, best of five:

| | time | against the scan |
|---|---|---|
| The scan, before | 294 ms | — |
| A run-time table in a `region`, hand-plumbed | 52 ms | 5.7× |
| **A `static` table** | **48 ms** | **6.1×** |
| GNU coreutils `base64 -d` | 14 ms | 21× |

All three agree with coreutils byte for byte.

The honest reading is the one §1 gives: **the table is the win, and this
feature is the way to have it without the plumbing or the arena.** The
gap between the last two rows is 4 ms on 5.4 MB — a 256-iteration loop
and one `malloc` at startup — and the instruction in the inner loop is
the same `mov` either way. Anyone reporting 6.1× as this feature's number
would be taking credit for the 5.7× that a `region` and four extra
arguments already buy.

What it did buy in that program, exactly:

- `value_of` keeps its signature, so it is still **pure** and
  `lex-sys authority` still says so. The plumbed version's `value_of`
  takes a `&t [int]`, which is also pure — but `decode`'s signature
  grows a region parameter, and `main` grows a `region` block.
- The table is in `.rodata`, 2 KB mapped from the file rather than
  dirtied at startup.
- Nothing is built at run time, so the program does the same work on a
  five-byte input as the plumbed one does on five megabytes minus 256
  iterations.

Where it is not merely convenience is §1.1's second row: 65 536 entries
does not fit in an arena, traps if you try, and needs `Heap` otherwise.
A `static` has no such ceiling, because the data is in the file rather
than in a chunk.

---

## 6. Open

| Question | Why it waits |
|---|---|
| Structs and enums in a `static` | §4. The evaluator's store holds scalars; an aggregate needs the layout rules the backend has and the evaluator does not. A table of pairs is the first thing anyone will want |
| A `static` that is a scalar | `static LIMIT: int { return 4 * 1024; }` is not allowed — the item is for data, and a scalar constant is what §3 of `compile-time.md` already folds. Worth revisiting only if a reader finds the asymmetry surprising |
| Nested slices | A `[[int]]` needs handles inside the store to survive emission as relocations. Real, and not needed by anything yet |
| Deduplicating identical statics | Two `static`s with the same bytes are two symbols today. The canonical AST makes the check cheap; nothing has asked |
| A bigger budget, or a stated one | §3 makes the fuel budget observable. It is a compiler constant; whether it should be a flag is a question about builds rather than about the language |

---

## 7. The suite

| Fixture | Rule | § |
|---|---|---|
| `static_alloc_outside_a_static.ls` | `alloc_slice[static]` is lexical to a `static` item | 2.1 |
| `unique_reference_into_a_static.ls` | `&!static` stays refused | 2.1 |
| `static_that_cannot_be_evaluated.ls` | Refuse, don't downgrade | 3 |
| `static_index_out_of_range.ls` | A certain trap while evaluating is a diagnostic | 4 |

| Test | Rule | § |
|---|---|---|
| `a_static_becomes_read_only_data` | The table is in the binary, and the program never builds it | 2 |
| `a_static_table_decodes_what_coreutils_decodes` | §5's rewrite agrees byte for byte | 5 |

| Accepting | Shows |
|---|---|
| `static_data.ls` | A table built by a loop, one built by calling a pure function, and both read back |
| `examples/base64/` | §1's 5.7×, without the plumbing §1 needed |
