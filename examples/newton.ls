// `newton` — a numerical method that Q16.16 could not carry.
//
// Newton's method for a square root: x <- (x + a/x) / 2, which doubles
// the number of correct digits per step. Four steps from a bad guess and
// it is at the limit of binary64; a fifth changes nothing because there
// is nothing left to change.
//
// The point is where it ends up. Fixed point at Q16.16 — what this
// language forced before `float` (`against-c-and-rust.md` §4) — resolves
// 1.526e-5, and **step 3's residual is already below that**. So the last
// three steps of this program are all invisible to the representation it
// used to have: the method converges either way, and only one of the two
// can show that it did.
//
// The residual also does not reach zero, which is the honest part. √2 is
// not representable in binary64, so the iteration settles on the nearest
// value that is, and `x * x - 2` bottoms out at about one unit in the
// last place near 2.0. A method that *converged* and a method that
// reached the answer are different things, and floating point is where
// the difference becomes visible.
//
// Every number below is printed by `std.fmt.float_into`
// (`float-printing.md`): the shortest decimal that reads back to the
// same bits, written in lex-sys rather than by the compiler. An earlier
// version of this file reported everything through `truncate` and a
// scale factor of 1e18, which was fixed point with extra steps — and the
// residuals it could show stopped at the point where the scale factor
// ran out, not where the method did: step 3's residual printed as
// `6007304882427` there and is `6.007304882427178e-6` here, and the
// three digits in the difference were the scale factor's fault.
//~ STDOUT step 1 x 1.5e0 residual 2.5e-1
//~ STDOUT step 2 x 1.4166666666666665e0 residual 6.944444444444198e-3
//~ STDOUT step 3 x 1.4142156862745097e0 residual 6.007304882427178e-6
//~ STDOUT step 4 x 1.4142135623746899e0 residual 4.510614104447086e-12
//~ STDOUT step 5 x 1.414213562373095e0 residual 4.440892098500626e-16
//~ STDOUT q16-16 floor 1.52587890625e-5
//~ STDOUT steps below that floor 3
//~ STDOUT hardware sqrt 1.4142135623730951e0
//~ STDOUT five steps are short by 1 ulp
//~ EXIT 0

import std.fmt;
import std.io;

// |x| without a `std.math` to take it from (`floating-point.md` §7).
fn magnitude(x: float) -> [] float {
    if x < 0.0 {
        return -x;
    }
    return x;
}

// One step. Pure, and `lex-sys authority` says so: its row is `[]` and
// it takes a `float` by value, which is §2 of `docs/purity.md`.
fn step(a: float, x: float) -> [] float {
    return (x + a / x) / 2.0;
}

// How far `x * x` is from `a` — the residual, which is what says the
// method converged rather than merely stopped moving.
fn residual(a: float, x: float) -> [] float {
    return magnitude(x * x - a);
}

// Print one float. The buffer is 24 bytes because `float-printing.md` §6
// says that is always enough, and it lives in a `region` because this
// program released its `Heap` before the first number existed.
// How many representable doubles lie between two positives.
//
// `bits_of` is a bitcast (`float-printing.md` §2), and for positive
// values the bit patterns increase with the value -- so subtracting them
// counts the representable numbers in between, which is what "one unit
// in the last place" means.
fn ulps_between(a: float, b: float) -> [] int {
    let x = bits_of(a);
    let y = bits_of(b);
    if x > y {
        return x - y;
    }
    return y - x;
}

fn show[&i](i: &!i Io, x: float) -> [io_write] int {
    region a {
        let out = alloc_slice[a](24, byte_of(0));
        let n = fmt.float_into(out, x);
        io.write_all(i, out[0..n]);
    }
    return 0;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);

    let a = 2.0;
    // A deliberately poor first guess, so the early steps have somewhere
    // to fall from.
    var x = 2.0;
    // One unit in Q16.16's last place, which is what the fixed-point
    // version of this program could resolve and no better.
    let floor = 1.0 / 65536.0;
    var below = 0;

    borrow mut io as &!i in {
        var n = 1;
        while n <= 5 {
            x = step(a, x);
            io.write_all(i, "step ");
            io.print_int(i, n);
            io.write_all(i, " x ");
            show(i, x);
            io.write_all(i, " residual ");
            show(i, residual(a, x));
            io.newline(i);
            if residual(a, x) < floor {
                below = below + 1;
            }
            n = n + 1;
        }

        // The floor Q16.16 would have hit: one unit in its last place is
        // 1/65536. Three of the five steps above finished under it.
        io.write_all(i, "q16-16 floor ");
        show(i, floor);
        io.newline(i);
        io.write_all(i, "steps below that floor ");
        io.print_int(i, below);
        io.newline(i);

        // What the method is being measured against. `sqrt` is a builtin
        // and one instruction, and IEEE-754 requires it to be correctly
        // rounded (`docs/float-math.md`) -- so this line is the exact
        // answer, and the five steps above end one unit in the last
        // place below it.
        //
        // That gap is the point of the program rather than a defect in
        // it: five steps of a method that doubles its digits get within
        // an ulp of a value the hardware computes outright, which says
        // more about how fast Newton converges than about how good the
        // instruction is.
        let exact = sqrt(a);
        io.write_all(i, "hardware sqrt ");
        show(i, exact);
        io.newline(i);
        io.write_all(i, "five steps are short by ");
        io.print_int(i, ulps_between(x, exact));
        io.write_all(i, " ulp");
        io.newline(i);
    }

    release(io);
    return 0;
}
