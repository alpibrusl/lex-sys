// `sieve` -- Eratosthenes, which is memory-bound.
//
// The inner loop is a strided byte write and an addition. The addition is
// checked; the write is a bounds check and a cache miss. This is what
// happens to the overflow check when it is not on the critical path.
//
// The limit is 60000 because an arena is one chunk (`heap.md`), and the
// answer is the number of primes below it.
//
// This is the **checked** half of a pair. Its twin differs only in using
// `wrapping_add`/`wrapping_sub`/`wrapping_mul`, which lower to a bare
// `iadd`/`isub`/`imul` with no trap — so the difference between the two
// binaries is exactly the cost of the guarantee, and nothing else.
//
// `python3 scripts/bench.py` runs the pair. Both return 0 when correct,
// which is how the suite checks that the two halves still agree.

fn run(limit: int, rounds: int) -> [] int {
    var found = 0;
    var r = 0;
    while r < rounds {
        region a {
            let mark = alloc_slice[a](limit, byte_of(0));
            var p = 2;
            while p * p < limit {
                if int_of(mark[p]) == 0 {
                    var m = p * p;
                    while m < limit {
                        mark[m] = byte_of(1);
                        m = m + p;
                    }
                }
                p = p + 1;
            }
            var i = 2;
            found = 0;
            while i < limit {
                if int_of(mark[i]) == 0 {
                    found = found + 1;
                }
                i = i + 1;
            }
        }
        r = r + 1;
    }
    return found - 6057;
}
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(io); release(ffi);
    return run(60000, 1000);
}
