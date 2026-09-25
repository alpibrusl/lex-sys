# `std.crypto`: SHA-256, and only SHA-256

> **Status: built, first slice of a larger initiative.**
>
> This is not a program-asked-for-it addition in the usual sense
> `AGENTS.md` §7 describes — there is no second `.ls` program in this
> repository that needed a hash function first. It exists because
> `ROADMAP.md`'s own investigation into porting `lex-os` off Rust found
> `crates/lex-os-audit`'s hash-chained log calls `sha2::Sha256` on every
> entry, and a hash-chained log with no hash function is not a smaller
> version of the port, it is a different thing. Named here rather than
> left implicit, because building ahead of a same-repo consumer is the
> exception `AGENTS.md` §7 does not otherwise make.

---

## 1. Scope: one algorithm, not a library

`std.crypto` is SHA-256 and nothing else — no SHA-1 (broken), no MD5
(broken), no AES, no TLS, no key exchange, no random-number generation.
Each of those is a separate, harder correctness problem with its own
attack surface, and none has a consumer yet. `lex-os-audit`'s own
dependency is exactly one function: `Sha256::new()` /
`.update()` / `.finalize()`, called once per log entry. `lex-os-capsule`
needs Ed25519 signing next, and that is a **second** slice, not this
one — hashing and signing are different primitives with different
failure modes, and bundling them because both start with "crypto" is
the kind of premature scope this document is refusing on purpose.

FIPS 180-4 is the spec. This implementation is checked against it by
the only method that means anything for a hash function: **byte-exact
output on real input**, not "the code looks like the pseudocode."

---

## 2. The arithmetic problem: SHA-256 is 32-bit, lex-sys's `int` is not

Every lex-sys `int` is a checked, 64-bit two's-complement value —
`docs/defined-behaviour.md`'s whole point is that `+`/`-`/`*` **trap**
on overflow rather than wrap. SHA-256 is specified entirely in
**32-bit modular arithmetic**: every addition in the compression
function is `mod 2^32` by design, not by accident, and a correct
implementation depends on that wraparound happening on schedule sixty
-four times per block.

The two do not conflict, once stated precisely: nothing in this
implementation lets a 32-bit *value* — always held as an `int` in the
range `[0, 0xffffffff]`, which is a small, unremarkable, very-non-negative
64-bit number — get anywhere near the checked-arithmetic ceiling.
`mask32(x) = x & 0xffffffff` after every addition throws away the bits
above 32 explicitly, in source, at the point the spec says to reduce
`mod 2^32` — so the checked `+` underneath never traps (the largest sum
this code ever forms is five 32-bit terms, ~2^34.3, nowhere near 2^63)
and the masking is doing the wraparound instead. No `wrapping_add` is
needed: the trap and the wraparound are solving two different problems
here, and this one only has the second.

The other consequence worth stating: `docs/bitwise.md` §2 makes `>>`
**arithmetic** (sign-extending), which would be wrong for SHA-256's
right shifts if they ever ran on a negative number. They never do —
every word this code shifts is already masked into `[0, 0xffffffff]`,
which as a 64-bit `int` is always positive, so arithmetic and logical
right shift agree on it. `rotr32` builds a 32-bit rotate out of two
opposite shifts and an `mask32`-ed `|`, not a dedicated rotate operator
this language does not have. `not32(x) = 0xffffffff - x` builds the
32-bit bitwise complement out of subtraction rather than lex-sys's own
`~` (which flips all 64 bits, not 32) — a true identity for any `x` in
`[0, 0xffffffff]`, not an approximation.

---

## 3. The round constants are a `static`, not a coincidence

FIPS 180-4's eight initial hash words and sixty-four round constants
are exactly `docs/compile-time-data.md`'s own motivating shape: fixed
data, known at compile time, read many times and written never. They
are declared as two `static` items (`sha256_h0`, `sha256_k`) rather
than as sixty-four `let` bindings recomputed per call or a parameter
threaded through every function — the first real consumer of `static`
outside its own design doc and `tests/accept/static_data.ls`, and
exactly the table shape `compile-time-data.md` §2 used `decode_table`
to illustrate.

---

## 4. The API

```
pub fn sha256[&s, &o](message: &s [byte], digest: &!o [byte]) -> [] int
```

`digest` is a caller-provided, unique-referenced output buffer, the
same "write into what the caller passed" shape `std.bignum`'s
`add_into`/`copy` already use rather than allocating and returning a
fresh slice — one write-only pointer, matching `crates/lex-os-audit`'s
own call shape, is what a caller reaches for. `digest` must be (at
least) 32 bytes; a shorter one traps on the ordinary bounds check
every other out-of-range write in this language already gets, which is
the correct, honest answer rather than a manual length precondition
duplicating it.

The row is `[]`. Nothing here performs an effect — `region`/
`alloc_slice` are not effects (`docs/linearity-and-effects.md` §7.3;
every effect needs a capability and this function takes none), and
reading a `static` performs nothing by the same rule that lets it have
row `[]` in the first place. A hash function that could not be called
from a pure context would be a strange hash function.

---

## 5. Checked, not assumed

Four vectors, none written from memory — computed with the system
`sha256sum` and matched against this implementation's own output
byte-for-byte:

| Input | Digest |
|---|---|
| `""` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `"abc"` | `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad` |
| `"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"` (56 bytes) | `248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1` |
| `"The quick brown fox jumps over the lazy dog"` | `d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592` |

The 56-byte vector is the one that matters most: `"" ` and `"abc"` both
pad to exactly one 64-byte block, so neither exercises the multi-block
path (`total + 9 > 64`, the padding pushing the message into a second
block) at all. 56 is the smallest input that does — `56 + 9 = 65 > 64`
— and it is the standard NIST test vector for exactly that reason, not
picked here for one.

`tests/accept/sha256.ls` checks all four, byte-for-byte, through
`--std`, on both backends via the normal accept-fixture path.

---

## 6. `static` was never gated by reachability, and this module found it

`AGENTS.md` §7's promise — *"a declaration nobody calls emits nothing"*
— is checked by `modules.rs`'s `std_declarations_cost_nothing_unless_
called`: it builds one program with `--std` and one without and asserts
the two object files are **byte-identical**. Adding `sha256_h0`/
`sha256_k` broke it on the first run, and the reason is a real gap
`static_data.ls` could never have exposed on its own: unlike a
function, a `static` was never a root `mono`'s reachability worklist
gated — `lib.rs` lowered and evaluated *every* `static` a program
declared, unconditionally, and pushed the result into `Program::
statics` whether or not any reachable function ever named it. A `std`
module with a function nobody calls already cost nothing, by
construction, before this slice; a `std` module with a `static`
nobody's reachable code reads did not, and nothing had ever declared
one to find out.

Fixed in `lex-sys-ir`, not in this module: `fold::collect_static_refs`/
`collect_static_refs_body` walk every `Expr` a reachable function holds
— exhaustively, with no wildcard arm, for the same reason `body/
expr.rs`'s own match lost its wildcard in the LLVM backend's §7.25 — and
record which `static` indices it actually names. Because a `static` may
only read one declared *before* it (`compile-time-data.md` §2's own
acyclic rule), closing over a used static's own body for *its* earlier
references is one pass per used index, not a fixed point search.
What survives is renumbered into a compact range and every `Expr::
Static` reference — in `program.funcs` and in a kept static's own body
alike — is rewritten to match before `evaluate_static` ever runs, so
its `evaluated: &[Vec<i64>]` lookup by that same index still lines up.
Checked with a static that reads another, earlier static (`static_data.
ls`'s own `doubled` reading `squares`, unaffected) and, freshly, with
three statics where the *middle* one is dropped and the third has to
renumber past it — both backends produce the arithmetically correct
answer, not just a program that links.

`static_data.ls` itself was never in a position to catch this: a
program written specifically to exercise a `static` also, necessarily,
reads it. The gap needed a `static` sitting in a library **nobody had
called yet**, which is exactly what this module's own status note —
built ahead of a same-repo consumer, rather than after one asked for
it — turned out to supply.

---

## 7. What is next

`lex-os-capsule`'s Ed25519 signing is the next real consumer, and it is
a separate slice: a different primitive, its own correctness surface
(a signature scheme has a private key to get right, a hash function
does not), and nothing here should be read as scoping it in advance.
`lex-os-guest`'s `vsock`/HTTP needs are further out still and belong to
`Net`/`Fs`, not `std.crypto`.
