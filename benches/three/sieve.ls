// Eratosthenes, the memory-bound half of the three-way comparison.
//
// The same algorithm as `sieve.c` and `sieve.rs`, and the same as
// `benches/sieve_checked.ls` — that one returns its answer as an exit
// status because it is only ever compared against its own wrapping twin,
// and this one prints it because the harness checks that all three
// languages computed the same number before it reports a time.
//
// Memory-bound where `mandelbrot` is compute-bound, which is what makes
// the pair worth having: one number is a benchmark, two are a shape.

import std.io;

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
    return found;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);

    let found = run(60000, 1000);
    borrow mut io as &!i in {
        io.print_nat(i, found);
        io.newline(i);
    }
    release(io);
    return 0;
}
