# Layout

> **Status: measured, and mostly answered "no".**
>
> `ROADMAP.md` listed struct layout as a place lex-sys could go **past**
> C: *"packing reaches C; array-of-structs to struct-of-arrays goes past
> it."* Half of that is right and the interesting half is wrong.
>
> Packing is real and this repository has nothing to spend it on (§2).
> Transposing does **not** go past C, because C gets the same 1.38× from
> it that lex-sys gets (§3) — measured, both directions. So this document
> is the measurement, the deferral with its trigger, and the correction.

---

## 1. What the layout is today

`defined-behaviour.md` §5: a value is scalarised into **leaves**, and
where it has to live in memory each leaf takes **8 bytes**. A `byte`
slice is the one exception, packed one per byte, so that a string is
something C could read.

```
struct Rgb { r: byte, g: byte, b: byte }     // 24 bytes. C's is 3.
```

Measured rather than derived: a 64 KiB arena holds **2730** of them,
which is 64 × 1024 / 24.

The same section already says the important thing about changing it:

> **It is not yet a stability contract.** Nothing depends on it across
> processes: no aggregate crosses the FFI boundary (§8.4 refuses one),
> nothing is serialised, and layout does not reach any hash.

So the compiler may pack, reorder and transpose whenever it likes. The
question this document asks is not *may we* but **is it worth
anything**.

---

## 2. Packing: real, and nothing here can spend it

### 2.1 What it is worth where it applies

A memory-bound traversal of 4 000 000 structs, eight passes, best of
five. The two rows differ only in the field type:

| struct | lex-sys | C `-O2` | ratio |
|---|---|---|---|
| `{ r, g, b: byte }` | 178 ms | 32 ms | **5.6×** |
| `{ r, g, b: int }` | 174 ms | 82 ms | **2.1×** |

Read those two rows against each other, because that is where the answer
is. **lex-sys's own time barely moves** — 178 against 174 — because its
layout does not change: 24 bytes either way. C's time moves by 2.6×,
because C's layout moves by 8× — 3 bytes against 24.

So on this shape the layout costs about **2.6×**, and it is the whole of
the difference between a 2.1× gap and a 5.6× one.

### 2.2 And this repository does not contain that shape

Every `struct` field declared in `tests/accept/`, `examples/` and
`std/`:

| | count |
|---|---|
| `int` | 60 |
| `bool` | 3 |
| everything else (references, boxes, type parameters, other structs) | 20 |
| **`byte` or `bool`** — the only ones packing shrinks | **3 of 83** |

And the one real struct that has a narrow field would not shrink at all:

```
struct Entry { generation: int, live: bool, value: int }    // examples/slab/
```

Packed and aligned that is 8 + 1 + 8 rounded back up to 24 — exactly
what it costs now. **Packing pays when narrow fields outnumber wide
ones**, and an `int` next to a `bool` pays for the `bool` twice over.

The honest summary is not "packing is worthless". It is:

> Packing is worth up to 2.6× on a struct that is mostly `byte` or
> `bool`, and no program in this repository is one.

That is `purity.md` §4.2's lesson applied before the code rather than
after it: a transformation nothing can exhibit is a transformation with
no test.

### 2.3 So it is deferred with a trigger rather than built

§4 is the instrument. `lex-sys layout` reports every type's size and
what it would be packed, so "would this help my program?" is a command
rather than a memory — and this deferral has a falsifier instead of a
promise. The first program here with an image, a packet header or a
token stream in it will make the report say so.

---

## 3. Transposing: a no, and the numbers say why

Array-of-structs to struct-of-arrays is the transform C **cannot** do
for you: struct layout is part of C's ABI, so a C programmer who wants
it rewrites the program. lex-sys promises no layout, so a compiler could
do it silently. That was the argument for it going *past* C.

The argument does not survive measurement. A loop touching **one field
of three**, 4 000 000 elements, eight passes, both transposed by hand:

| | array of structs | struct of arrays | gain |
|---|---|---|---|
| lex-sys | 125 ms | 87 ms | **1.43×** |
| C `-O2` | 72 ms | 55 ms | **1.31×** |

**The transform is worth about the same in both languages.** It does not
move lex-sys past C; it moves lex-sys and C along together, and the gap
between them stays what it was.

Two reasons the memory-traffic argument oversells it:

- The naive model says reading 8 bytes per element instead of 24 should
  be 3×. It is not, because at these sizes both versions are streaming
  from RAM and the loop is bound by its own instructions — a
  bounds-checked load, an add and a compare — rather than by bandwidth.
- Where the model *would* be right is a vectorised loop, and
  `overflow-cost.md` §3.2 already established that lex-sys does not
  vectorise: a trapping add is not reassociable. So the half of the
  prize that needs contiguity-plus-SIMD is a half this backend cannot
  collect even if the layout were perfect.

And the cost is not small. Choosing AoS or SoA per type needs to know
**which fields each loop touches**, which is a whole-program analysis
this compiler does not have and would have to grow — for 1.4× on a shape
where C gets 1.3× by hand.

> So: **no.** Not "later", not "when the backend is better" — the
> measurement says the transform is not a lex-sys advantage at all, and
> the roadmap entry claiming it was is corrected rather than deferred.

---

## 4. `lex-sys layout`

```
$ lex-sys layout examples/slab/slab.ls examples/slab/main.ls --std
type                     leaves    size   packed   stride
Gen                           2      16       16       16
Entry                         3      24       24       24
Slab                          3      24       24       24
Found                         2      16       16       16
Buffer                        3      24       24       24
```

Every `packed` equals every `size`, which is §2.2 as a command rather
than as a paragraph: this program has nothing to gain. The same report on
a struct that does:

```
$ lex-sys layout rgb.ls --std
type                     leaves    size   packed   stride
Rgb                           3      24        3       24
Buffer                        3      24       24       24

packing would save 21 bytes per copy across these types (`docs/layout.md` §2)
```

- **leaves** is the scalarised count `defined-behaviour.md` §5 defines.
- **size** is what it costs in memory today: leaves × 8, except `byte`.
- **packed** is what it would cost with each leaf at its natural width
  and fields reordered widest-first, aligned. When these two columns
  differ, §2 applies to your program.
- **stride** is the distance between elements of a `[T]`, which is the
  number a traversal actually pays.

It is a report rather than a flag because there is nothing to turn on:
§2 did not change the layout, it measured it.

---

## 5. What this does not touch

- **The `byte` exception stays.** `[byte]` is packed one per byte and
  that is a promise `strings.md` §3 makes for a reason: a string has to
  be something C could read, and `examples/base64/` and
  `examples/serve/` both depend on it.
- **Field order stays declaration order.** Reordering is only worth
  anything once leaves have different widths, so it arrives with
  packing or not at all.
- **Nothing becomes a contract.** `defined-behaviour.md` §5's paragraph
  stands unchanged: layout is deterministic, not stable, and the day
  something depends on it across processes is the day it moves to
  `canonical-ast.md`.

---

## 6. Open

| Question | Why it waits |
|---|---|
| Packing narrow leaves | §2.3. Deferred with a trigger, not a date: when `lex-sys layout`'s `size` and `packed` columns differ on a program someone cares about |
| A vectoriser, or a backend with one | §3's second reason. It gates the half of the transposing prize that is real, and it is the same wall `overflow-cost.md` §3.2 hit — so it is one problem wearing two hats, not two |
| Enum tag width | A tag is one leaf, so 8 bytes, whatever the variant count. Packing would make it 1 for any enum under 256 variants. Folded into §2 rather than separate: it is the same change and the same trigger |
| Layout as a contract | §5. Nothing needs it yet. `reach.md` §3.2 keeps aggregates out of FFI, which is what keeps it free |

---

## 7. The suite

| Test | Rule | § |
|---|---|---|
| `the_layout_report_says_what_a_type_costs` | Sizes, strides and the packed column agree with what the backend emits | 4 |

| Bench | Shows |
|---|---|
| `benches/layout/` | §2's two rows and §3's four, as programs rather than as numbers in a document |
