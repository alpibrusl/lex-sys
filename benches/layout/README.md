# `benches/layout/`

`docs/layout.md`'s numbers, as programs rather than as a table someone
typed. Two questions, four pairs.

## The width question (§2)

Does storing every leaf in 8 bytes cost anything?

| | |
|---|---|
| `rgb.ls` / `rgb.c` | `struct { r, g, b: byte }` — 24 bytes here, 3 in C |
| `ints.ls` / `ints.c` | `struct { r, g, b: int }` — 24 bytes in **both** |

The pair is the measurement: lex-sys's two times are nearly equal
because its layout does not change, and C's differ by 2.6× because C's
does. Everything else about the two programs is identical.

## The shape question (§3)

Is array-of-structs to struct-of-arrays a lex-sys advantage?

| | |
|---|---|
| `aos.ls` / `aos.c` | One array of `{x, y, z: int}`, a loop touching only `.x` |
| `soa.ls` / `soa.c` | Three arrays, the same loop |

Transposed **by hand** in both languages, because the answer turned out
to be that a compiler doing it automatically would not be doing anything
a C programmer cannot do in an afternoon — and would gain about as
much. §3 is the four numbers and what they settle.

## Running them

```sh
cargo build --release
for f in rgb ints aos soa; do
    ./target/release/lex-sys build --std benches/layout/$f.ls -o /tmp/$f
    cc -O2 benches/layout/$f.c -o /tmp/${f}_c
done
```

Each pair prints the same checksum, which is what makes them comparable
rather than merely similar.
