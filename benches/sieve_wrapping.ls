// `sieve` -- Eratosthenes, which is memory-bound.
//
// The inner loop is a strided byte write and an addition. The addition is
// checked; the write is a bounds check and a cache miss. This is what
// happens to the overflow check when it is not on the critical path.
//
// The limit is 60000 because an arena is one chunk (`heap.md`), and the
// answer is the number of primes below it.
//
// This is the **wrapping** half of a pair: the same program with the
// traps removed, which is the baseline the checked half is measured
// against. It is not idiomatic lex-sys and is not meant to be —
// `wrapping_add` means *the bits are the intent* (`defined-behaviour.md`
// §2.2), and here the intent is only to delete the check.
//
// `python3 scripts/bench.py` runs the pair.

fn run(limit: int, rounds: int) -> [] int {
    var found = 0;
    var r = 0;
    while r < rounds {
        region a {
            let mark = alloc_slice[a](limit, byte_of(0));
            var p = 2;
            while wrapping_mul(p, p) < limit {
                if int_of(mark[p]) == 0 {
                    var m = wrapping_mul(p, p);
                    while m < limit {
                        mark[m] = byte_of(1);
                        m = wrapping_add(m, p);
                    }
                }
                p = wrapping_add(p, 1);
            }
            var i = 2;
            found = 0;
            while i < limit {
                if int_of(mark[i]) == 0 {
                    found = wrapping_add(found, 1);
                }
                i = wrapping_add(i, 1);
            }
        }
        r = wrapping_add(r, 1);
    }
    return wrapping_sub(found, 6057);
}
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(io); release(ffi);
    return run(60000, 1000);
}
