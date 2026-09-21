// fannkuch-redux, from the Computer Language Benchmarks Game.
//
// Integer arrays and nothing else: permutations generated in the order
// the benchmark specifies, each one flipped until its first element is
// 1, counting flips. No heap, no floats, no IO until the end.
//
// The permutation order is part of the program rather than an
// implementation choice -- the checksum alternates sign by permutation
// *index*, so a different order gives a different answer. Generating
// them lexicographically produces -502 for n=7 where the benchmark
// says 228, which is how this was caught.
//
//~ STDOUT 228
//~ STDOUT Pfannkuchen(7) = 16
//~ EXIT 0

import std.bytes;
import std.io;

fn run[&i](out: &!i Io, n: int) -> [io_write] int {
    var checksum = 0;
    var maxflips = 0;

    region a {
        let perm = alloc_slice[a](n, 0);
        let perm1 = alloc_slice[a](n, 0);
        let count = alloc_slice[a](n, 0);

        var i = 0;
        while i < n {
            perm1[i] = i;
            i = i + 1;
        }

        var r = n;
        var permcount = 0;
        var done = false;
        while !done {
            while r != 1 {
                count[r - 1] = r;
                r = r - 1;
            }

            var j = 0;
            while j < n {
                perm[j] = perm1[j];
                j = j + 1;
            }

            var flips = 0;
            var k = perm[0];
            while k != 0 {
                // Reverse `perm[0..k+1]` in place.
                var lo = 0;
                var hi = k;
                while lo < hi {
                    let swap = perm[lo];
                    perm[lo] = perm[hi];
                    perm[hi] = swap;
                    lo = lo + 1;
                    hi = hi - 1;
                }
                flips = flips + 1;
                k = perm[0];
            }

            if flips > maxflips {
                maxflips = flips;
            }
            // The sign alternates by permutation index, which is why the
            // order above is the benchmark's and not any order.
            if permcount - (permcount / 2) * 2 == 0 {
                checksum = checksum + flips;
            } else {
                checksum = checksum - flips;
            }

            // Next permutation, by the rotation the benchmark specifies.
            var rotating = true;
            while rotating {
                if r == n {
                    rotating = false;
                    done = true;
                } else {
                    let first = perm1[0];
                    var m = 0;
                    while m < r {
                        perm1[m] = perm1[m + 1];
                        m = m + 1;
                    }
                    perm1[r] = first;
                    count[r] = count[r] - 1;
                    if count[r] > 0 {
                        rotating = false;
                    } else {
                        r = r + 1;
                    }
                }
            }
            permcount = permcount + 1;
        }
    }

    io.print_int(out, checksum);
    io.newline(out);
    io.write_all(out, "Pfannkuchen(");
    io.print_int(out, n);
    io.write_all(out, ") = ");
    io.print_int(out, maxflips);
    io.newline(out);
    return 0;
}

// The benchmark's `N`, from the command line, with the verified default
// this file's header states. `std.bytes` has `digit_of` and no whole
// number parser (`standard-library.md` §3.1), so it is four lines here
// rather than a library addition nothing else has asked for.
fn size_from[&g](args: &g Args, fallback: int) -> [args] int {
    if arg_count(args) < 2 {
        return fallback;
    }
    let text = arg(args, 1);
    var value = 0;
    var i = 0;
    while i < len(text) {
        let digit = bytes.digit_of(int_of(text[i]));
        if digit < 0 {
            return fallback;
        }
        value = value * 10 + digit;
        i = i + 1;
    }
    if value <= 0 {
        return fallback;
    }
    return value;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(heap);
    release(fs);
    release(ffi);
    var n = 7;
    borrow args as &g in {
        n = size_from(g, 7);
    }
    release(args);
    borrow mut io as &!i in {
        run(i, n);
    }
    release(io);
    return 0;
}
