// Mandelbrot, Q16.16 fixed point -- the numerical benchmark, and the one
// that says what having no `float` costs.
//
// The same algorithm exists here as `mandelbrot.c` and `mandelbrot.rs`,
// and all three must print the same checksum: a numerical kernel where
// the three languages disagree is not a benchmark, it is three
// benchmarks.
//
// Fixed point because there is no `float` (`docs/reach.md` §2). Q16.16 in
// a 64-bit integer: a value is the real number times 65536, a product is
// `(a * b) >> 16`, and nothing here overflows because |z| < 2 before the
// escape test fires -- which is worth saying out loud, since `*` traps
// and a benchmark that trapped would be a different kind of result.
//
// The precision is ~1.5e-5, against f64's 2.2e-16. That is the other
// half of the cost and it does not appear in the timing at all.

import std.io;

// 4.0 in Q16.16: the escape radius squared.
fn limit() -> [] int {
    return 4 << 16;
}

// How many iterations before |z| escapes, up to `maxiter`.
//
// There is no `break`, so the escape test is in the loop condition and
// the squares are carried across iterations rather than recomputed --
// which is what the C would do anyway.
fn escape(cx: int, cy: int, maxiter: int) -> [] int {
    var zx = 0;
    var zy = 0;
    var zx2 = 0;
    var zy2 = 0;
    var i = 0;
    while i < maxiter && zx2 + zy2 <= limit() {
        let zxy = (zx * zy) >> 16;
        zx = zx2 - zy2 + cx;
        zy = 2 * zxy + cy;
        zx2 = (zx * zx) >> 16;
        zy2 = (zy * zy) >> 16;
        i = i + 1;
    }
    return i;
}

// The whole grid, summed. The sum is the checksum the other two must
// match, and it is also why nothing here can be optimised away.
fn grid(width: int, height: int, maxiter: int) -> [] int {
    var total = 0;
    var py = 0;
    while py < height {
        // cy from -1.25 to +1.25
        let cy = ((py * 163840) / height) - 81920;
        var px = 0;
        while px < width {
            // cx from -2.0 to +0.5
            let cx = ((px * 163840) / width) - 131072;
            total = total + escape(cx, cy, maxiter);
            px = px + 1;
        }
        py = py + 1;
    }
    return total;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);

    let total = grid(400, 400, 1000);

    // Printed rather than returned, because an exit status is one byte
    // and this is forty million. It is also what the other two print, so
    // the harness can refuse to report a time for builds that disagree.
    borrow mut io as &!i in {
        io.print_nat(i, total);
        io.newline(i);
    }
    release(io);
    return 0;
}
