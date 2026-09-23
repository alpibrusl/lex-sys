//~ STDIN the quick brown fox jumps over the lazy dog
//~ STDOUT 4.442275084

// `entropy` — the Shannon entropy, in bits, of standard input's byte
// distribution.
//
// The second asker for `std.math.log` (`entropy.md`... there is no such
// document; the design question was already settled at `float-math.md`
// §6 -- `docs/ROADMAP.md`'s row for it names this file and
// `growth.ls`). `count_byte` counts one value; this needs all 256 at
// once, so it keeps its own histogram rather than reaching for
// `std.bytes`.
//
// H = -sum(p * log2(p)) over every byte value that occurred, where
// log2(p) is log(p) / log(2) -- there is no `log2`, and one call bought
// with a division is not worth a fourth function nobody else has asked
// for.

import std.io;
import std.math;

fn print_nine[&i](out: &!i Io, x: float) -> [io_write] int {
    let scaled = truncate(x * 1.0e9 + 0.5);
    let whole = scaled / 1000000000;
    var rest = scaled - whole * 1000000000;
    io.print_int(out, whole);
    io.write_all(out, ".");
    var place = 100000000;
    while place > 0 {
        io.print_int(out, rest / place);
        rest = rest - (rest / place) * place;
        place = place / 10;
    }
    io.newline(out);
    return 0;
}

fn entropy[&i](term: &!i Io) -> [io_read] float {
    var result = 0.0;
    region a {
        let counts = alloc_slice[a](256, 0);
        var total = 0;
        var c = getchar(term);
        while c >= 0 {
            counts[c] = counts[c] + 1;
            total = total + 1;
            c = getchar(term);
        }

        let ln2 = math.log(2.0);
        var h = 0.0;
        var i = 0;
        while i < 256 {
            if counts[i] > 0 {
                let p = float_of(counts[i]) / float_of(total);
                h = h - p * (math.log(p) / ln2);
            }
            i = i + 1;
        }
        result = h;
    }
    return result;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(heap);
    release(fs);
    release(ffi);
    release(args);
    borrow mut io as &!i in {
        let h = entropy(i);
        print_nine(i, h);
    }
    release(io);
    return 0;
}
