# Strings

> **Status: settled and built.** This was the gating artifact for M3's last
> item ([#1](https://github.com/alpibrusl/lex-sys/issues/1)), written and
> reviewed before the code the way `linearity-and-effects.md` was written
> before M2. §9's must-reject suite was stated in advance and is now
> enforced, fixture by fixture.
>
> **One thing changed on contact.** §4 said the static region had "nothing
> to name" and so was not writable in source. Writing the first fixture
> showed that this makes a literal impossible to *return* — `fn greeting()
> -> [] &static [byte]` has nowhere to put the region — which is a basic
> thing to want and nothing was buying the restriction. So `static` is
> writable in a type, and reserved as a binder: it is the one region with a
> name rather than a binder.

Everything else in M3 is built: arenas, libc FFI, checked arithmetic, slices,
the canonical printer, per-unit identity. Strings are what is left, and they
are the one item whose *representation* is still an open question rather than
an implementation.

---

## 1. What a string is

> **A string is a run of bytes. No encoding is claimed, and none is
> validated.**

`str` does not exist as a type. There is `byte`, and a string is `&r [byte]`
— an ordinary slice, which means it is an ordinary reference, which means
every rule §5 of `linearity-and-effects.md` gave references applies to it
without a second mechanism. That is the same trade slices made and it paid
off there: regions, the escape check, the unique-to-shared coercion and `val`
mode all came for free.

**Why not UTF-8-validated.** A validated string type has to answer what an
invalid one *is* — an error, a replacement character, an unrepresentable
state — and each answer costs either a fallible constructor everywhere or a
lie somewhere. Bytes answer it by not asking: there is no invalid byte
string, so `defined-behaviour.md` §8's open question ("Strings — encoding,
and what an invalid one is") is closed by having no encoding to be invalid
against.

Validation and decoding belong in a library, written *in* lex-sys, over
`&r [byte]`. That is the "slice-shaped, not a port of `std.str`" the epic
asks for.

**Why not a distinct `str` referent.** It was the other candidate: make
`str` an unsized shape like `[T]`, with indexing that yields `int`. It
avoids adding a type — but it needs its own construction form, its own
indexing rule, its own length rule and its own FFI rule, all parallel to the
slice ones and none shared with them. `byte` costs one type and reuses
everything.

---

## 2. `byte` is storage, not arithmetic

> **`byte` is an 8-bit unsigned integer with no arithmetic. It is converted
> to `int` to be computed with, and back to be stored.**

```
byte_of(n: int) -> [] byte      // traps unless 0 <= n <= 255
int_of(b: byte) -> [] int       // always defined, always 0..255
```

This is the decision that keeps the type small. `defined-behaviour.md` §8
defers unsigned integers and other widths because they drag conversion rules
and a second overflow question behind them — and a `byte` you cannot add to
never asks either. `b + 1` is refused; `byte_of(int_of(b) + 1)` is what you
write, and it says where the range check happens.

`==` and `!=` on bytes are allowed: comparing storage is not arithmetic, and
a parser that cannot say `b == byte_of(44)` is not worth having.

**`byte_of` traps rather than truncating.** Truncation is the silently wrong
answer `defined-behaviour.md` §2.1 already refused for `+`. A caller that
*wants* the low eight bits writes the mask explicitly, once there are
bitwise operators to write it with.

---

## 3. `[byte]` is packed

A slice's elements are one leaf-stride apart, and a leaf is 8 bytes
(`defined-behaviour.md` §5). `[byte]` is the exception: **its elements are
one byte apart**, because a string that took 8 bytes per character could not
be handed to C and would not be a string so much as a rumour of one.

So the layout rule generalises: an element's stride is its *size*, which is 1
for `byte` and the leaf count times 8 for everything else. This is the one
place M3 introduces a size that is not a multiple of 8, and it is confined to
`byte` on purpose — the day a second such type appears is the day the layout
section of `defined-behaviour.md` stops being a paragraph and becomes a
contract.

---

## 4. Literals, and where they live

```
let greeting = "Hello, world!\n";     // &static [byte]
```

A literal's bytes are emitted into the object file's read-only data and the
slice points at them. That needs a region that outlives every other, so:

> **`Region::Static` joins `Param`, `Block` and `Var`. It outlives
> everything and nothing outlives it.**

One row in the outlives relation (`Static` outlives all; nothing outlives
`Static` but itself), and the escape check needs no change at all — a
reference into the static region never mentions a block, so it never escapes
anything.

**`static` is writable in a type and reserved as a binder.** `&static
[byte]` is how a function says it hands back a literal; `fn f[&static]` and
`&!static` are both refused, the first because the region is not a
parameter anyone declares and the second because there is no unique
reference into read-only data.

String literals are **shared**, never unique: two occurrences of `"ok"` may
be the same bytes, and a program that could write through one would be
writing through both. `&static [byte]` it is.

**Escapes.** The set is `\n`, `\r`, `\t`, `\\`, `\"` and `\0`, and nothing
else. No `\u`, because that is an encoding claim (§1), and no `\x`, because
that is the bitwise escape hatch §2 is deferring. A backslash before
anything else is refused where it is written rather than passed through.

> **`\r` was added after M3, and by a program rather than by a list.** The
> original five left it out because nothing had asked for it, which was true
> right up until `examples/serve/` spoke HTTP. A protocol's line ending is
> CRLF, so without `\r` the separator had to be assembled as `byte_of(13)`
> into a buffer — the one piece of that program that was awkward for no
> reason anybody could defend (`docs/reach.md` §4). It is a byte with a
> spelling, not an encoding claim, so it belongs with `\t` rather than with
> `\u`.

---

## 5. Building one

An arena already allocates slices, and a string buffer is a slice:

```
region a {
    let buffer = alloc_slice[a](64, byte_of(0));   // &!a [byte]
    buffer[0] = byte_of(72);
}
```

Nothing new. `alloc_slice`, `len`, `s[i]`, `s[i] = v` and the bounds checks
all work on `[byte]` the moment `byte` exists, which is the point of §1's
choice.

What M3 does **not** add: concatenation, formatting, growth, interning, or a
`String` that owns its buffer. Those are library work over slices, and the
first three need an allocator story beyond one arena chunk.

---

## 6. What crosses to C

`linearity-and-effects.md` §8.4 lets `int`, `bool`, `()` and borrowed
capabilities cross. Strings add one:

> **A `&r [byte]` crosses as a pointer, and its length crosses as a separate
> `int` argument.** The two are separate parameters on the C side, because C
> has no notion of the pair.

So `write(fd, ptr, len)` is expressible and `strlen(ptr)` is not — the second
needs a NUL terminator, and this design does not put one there. A program
that wants a C string builds one, with the `\0` it chose, and takes
responsibility for the byte it wrote.

That asymmetry is deliberate: the functions that take an explicit length are
the ones that cannot run off the end, and preferring them is the same
argument the bounds check makes.

---

## 7. What this unblocks

The epic's M3 acceptance is "a real ~500-line program (a working CLI tool
doing file IO and parsing)". After this, what is still missing for that is
**file IO**, which needs an `Fs` capability — `linearity-and-effects.md` §8.1
lists it as waiting for exactly this reason, and it is M3's honest last mile
rather than a surprise.

---

## 8. Open, and deliberately so

| Question | Why it waits |
|---|---|
| Bitwise operators on `int` | Needed for masking, and `defined-behaviour.md` §8 wants shift semantics settled with them |
| ~~A growable buffer~~ | **Done** — `boxed-slices.md` gave it an allocator that is not one arena chunk, and `std.buffer` is the buffer. `slicing.md` §6 added `bytes`, which is how one crosses to anything taking a `&r [byte]` |
| UTF-8 decoding | Library work over `[byte]`, in lex-sys, once there is enough language to write it |
| `\x` and `\u` escapes | Each is a claim this design declines to make (§4) |
| Interning literals | An optimisation, and one that changes whether two literals share an address — which is observable, so it needs a rule before it needs code |

---

## 9. The must-reject suite

The deliverable that matters, stated before the code exists — the same shape
as `linearity-and-effects.md` §11, which is the part of M2 that made the
difference.

| Fixture | Rule | § |
|---|---|---|
| `byte_out_of_range.ls` | `byte_of(256)` traps rather than truncating | 2 |
| `byte_arithmetic.ls` | `b + 1` is refused; a byte is storage | 2 |
| `string_literal_is_shared.ls` | A literal may not be written through | 4 |
| `unknown_escape.ls` | `"\q"` is refused where it is written | 4 |
| `string_escapes_its_region.ls` | A buffer in an arena does not outlive it | 5 |
| `c_string_without_terminator.ls` | A `&r [byte]` is not a C string | 6 |

And the accepting counterparts, because a rule that rejects everything is not
a rule:

| Fixture | Shows |
|---|---|
| `string_literal.ls` | A literal printed a byte at a time, with no packing |
| `string_buffer.ls` | A buffer built in an arena, written, read back |
| `bytes_to_c.ls` | A pointer and a length crossing to a C function that takes both |

`examples/hello.ls` stops packing its greeting into two 64-bit words, which
is the single clearest signal that this landed: that file had been carrying
an M0 workaround since the first milestone, and the comment at the top of it
said so. It now writes `"Hello, world!\n"`.

`examples/wordcount.ls` is the first program in the repo that is mostly text
processing rather than demonstration: `wc` over an embedded document, with a
whole-word search that is two slices compared a byte at a time — which is
all a string comparison is when a string is bytes. It counts bytes and ASCII
whitespace and says in its own header that it is not Unicode-aware, because
§1 claims no encoding.
