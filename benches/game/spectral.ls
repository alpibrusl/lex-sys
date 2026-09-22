// spectral-norm, from the Computer Language Benchmarks Game.
//
// Floating point and nothing else: the largest eigenvalue of an
// infinite matrix, by ten rounds of the power method. Every number is a
// `float`, and the answer is printed to nine decimal places, which is
// what makes this a check on the arithmetic rather than on the loop.
//
// `sqrt` is a builtin now (`docs/float-math.md`), and this file is why
// the slice happened: the twenty-step Newton loop that used to sit here
// was wrong on 58.4% of values in the last place and **wrong by 143
// orders of magnitude** on a large one -- 10^300 came back as
// 4.77 x 10^293. The benchmark's answer never noticed, because
// spectral-norm only ever asks for the root of something near 1.27.
//
//~ STDOUT 1.274219991
//~ EXIT 0

import std.bytes;
import std.io;

// A(i, j) = 1 / ((i + j)(i + j + 1)/2 + i + 1)
fn eval_a(i: int, j: int) -> [] float {
    let sum = i + j;
    return 1.0 / float_of(sum * (sum + 1) / 2 + i + 1);
}

fn multiply_av[&v, &o](n: int, v: &v [float], out: &!o [float]) -> [] int {
    var i = 0;
    while i < n {
        var sum = 0.0;
        var j = 0;
        while j < n {
            sum = sum + eval_a(i, j) * v[j];
            j = j + 1;
        }
        out[i] = sum;
        i = i + 1;
    }
    return 0;
}

fn multiply_atv[&v, &o](n: int, v: &v [float], out: &!o [float]) -> [] int {
    var i = 0;
    while i < n {
        var sum = 0.0;
        var j = 0;
        while j < n {
            sum = sum + eval_a(j, i) * v[j];
            j = j + 1;
        }
        out[i] = sum;
        i = i + 1;
    }
    return 0;
}

fn multiply_atav[&v, &o, &t](
    n: int,
    v: &v [float],
    out: &!o [float],
    scratch: &!t [float],
) -> [] int {
    multiply_av(n, v, scratch);
    multiply_atv(n, scratch, out);
    return 0;
}

// Nine decimal places, which is the benchmark's output format and not
// one `std.fmt` has: `float_into` prints the *shortest* decimal that
// round-trips (1.2742199912349306e0 here), and a fixed precision is
// `float-printing.md` §7's open row. So the program carries its own,
// which is itself a finding -- see `benchmarks-game.md` §3.
fn print_nine[&i](out: &!i Io, x: float) -> [io_write] int {
    // 1.27e9 is nowhere near `int`'s range, so the scaling is exact
    // enough and the rounding is the benchmark's own.
    let scaled = truncate(x * 1.0e9 + 0.5);
    let whole = scaled / 1000000000;
    var rest = scaled - whole * 1000000000;
    io.print_int(out, whole);
    io.write_all(out, ".");
    // Zero-padded to nine, most significant first.
    var place = 100000000;
    while place > 0 {
        io.print_int(out, rest / place);
        rest = rest - (rest / place) * place;
        place = place / 10;
    }
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

    var n = 100;
    borrow args as &g in {
        n = size_from(g, 100);
    }
    release(args);
    var answer = 0.0;
    region a {
        let u = alloc_slice[a](n, 1.0);
        let v = alloc_slice[a](n, 0.0);
        let scratch = alloc_slice[a](n, 0.0);

        var round = 0;
        while round < 10 {
            multiply_atav(n, u, v, scratch);
            multiply_atav(n, v, u, scratch);
            round = round + 1;
        }

        var vbv = 0.0;
        var vv = 0.0;
        var i = 0;
        while i < n {
            vbv = vbv + u[i] * v[i];
            vv = vv + v[i] * v[i];
            i = i + 1;
        }
        answer = sqrt(vbv / vv);
    }

    borrow mut io as &!i in {
        print_nine(i, answer);
    }
    release(io);
    return 0;
}
