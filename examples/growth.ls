//~ STDOUT 1648.721271
//~ STDOUT 1628.894627
//~ STDOUT 13.862944

// `growth` — continuous and discrete compound growth over a principal,
// a rate and a number of periods, plus the doubling time the rate
// implies.
//
// The second asker for `std.math.exp` and the third for `log`
// (`docs/ROADMAP.md`'s row for this slice names this file,
// `entropy.ls` and `decay.ls`). Continuous growth is `exp` because it is
// what `exp` is *for* -- the solution to `dy/dt = r*y` -- and discrete
// growth the same amount compounded once a period is `pow`, the two
// answering slightly different questions on the same three numbers
// rather than one program computing one line and calling it done.

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
    io.newline(out);
    return 0;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(heap);
    release(fs);
    release(ffi);
    release(args);

    let principal = 1000.0;
    let rate = 0.05;
    let periods = 10.0;

    borrow mut io as &!i in {
        // Continuously compounded: P * e^(rt).
        print_six(i, principal * math.exp(rate * periods));
        // Compounded once per period instead: P * (1+r)^t.
        print_six(i, principal * math.pow(1.0 + rate, periods));
        // How long at this rate before the principal alone doubles:
        // ln(2) / r, independent of how much there started out being.
        print_six(i, math.log(2.0) / rate);
    }
    release(io);
    return 0;
}
