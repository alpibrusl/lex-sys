//~ STDOUT 1000.000000 1000.000000
//~ STDOUT 500.000000 500.000000
//~ STDOUT 250.000000 250.000000
//~ STDOUT 125.000000 125.000000
//~ STDOUT 62.500000 62.500000
//~ STDOUT 31.250000 31.250000

// `decay` — a radioactive decay table, each row computed two ways: once
// through `exp` and once through `pow`, printed side by side so a
// reader can see they agree rather than take it on faith.
//
// The third asker for `exp` and second for `pow`
// (`docs/ROADMAP.md`'s row for this slice names this file, `growth.ls`
// and `entropy.ls`). N(t) = N0 * e^(-ln(2)*t/half_life) is the
// differential-equation form every derivation starts from; N(t) = N0 *
// 0.5^(t/half_life) is the same law read off the definition of a
// half-life directly. They are one number, not two, which is the point
// of printing both.

import std.io;
import std.math;

fn print_six[&i](out: &!i Io, x: float) -> [io_write] int {
    let scaled = truncate(x * 1.0e6 + 0.5);
    let whole = scaled / 1000000;
    var rest = scaled - whole * 1000000;
    io.print_int(out, whole);
    io.write_all(out, ".");
    var place = 100000;
    while place > 0 {
        io.print_int(out, rest / place);
        rest = rest - (rest / place) * place;
        place = place / 10;
    }
    return 0;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(heap);
    release(fs);
    release(ffi);
    release(args);

    let n0 = 1000.0;
    let half_life = 5.0;
    let ln2 = math.log(2.0);

    borrow mut io as &!i in {
        var t = 0.0;
        while t <= 25.0 {
            let via_exp = n0 * math.exp(0.0 - ln2 * t / half_life);
            let via_pow = n0 * math.pow(0.5, t / half_life);
            print_six(i, via_exp);
            io.space(i);
            print_six(i, via_pow);
            io.newline(i);
            t = t + 5.0;
        }
    }
    release(io);
    return 0;
}
