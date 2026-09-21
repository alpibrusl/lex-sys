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
// value that is, and `x * x - 2` bottoms out at 4.44e-16 — about one
// unit in the last place near 2.0. A method that *converged* and a
// method that reached the answer are different things, and floating
// point is where the difference becomes visible.
//
// Reported through `truncate` and a scale factor, because printing a
// float is `floating-point.md` §7's open question. That is awkward and
// the document says so rather than pretending otherwise.
//~ STDOUT step 1 residual*1e18 250000000000000000
//~ STDOUT step 2 residual*1e18 6944444444444198
//~ STDOUT step 3 residual*1e18 6007304882427
//~ STDOUT step 4 residual*1e18 4510614
//~ STDOUT step 5 residual*1e18 444
//~ STDOUT q16-16 floor*1e18 15258789062500
//~ STDOUT steps below that floor 3
//~ EXIT 0

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
            io.write_all(i, " residual*1e18 ");
            // Scaled and truncated, which is the only way to show a float
            // today. 1e18 keeps the smallest residual a whole number and
            // the largest inside `int`.
            io.print_int(i, truncate(residual(a, x) * 1.0e18));
            io.newline(i);
            if residual(a, x) < floor {
                below = below + 1;
            }
            n = n + 1;
        }

        // The floor Q16.16 would have hit: one unit in its last place is
        // 1/65536. Three of the five steps above finished under it.
        io.write_all(i, "q16-16 floor*1e18 ");
        io.print_int(i, truncate(floor * 1.0e18));
        io.newline(i);
        io.write_all(i, "steps below that floor ");
        io.print_int(i, below);
        io.newline(i);
    }

    release(io);
    return 0;
}
