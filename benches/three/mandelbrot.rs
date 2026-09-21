// The same algorithm as `mandelbrot.ls`, in Rust.
//
// Built twice: `-C overflow-checks=on` matches lex-sys's semantics, and
// off is Rust's own release default, which wraps. The gap between those
// two rows is the same gap `docs/overflow-cost.md` measured, paid by a
// different compiler.
//
// `>>` on a signed integer is arithmetic in Rust, which is defined
// rather than implementation-defined as it is in C.

const LIMIT: i64 = 4 << 16;

#[inline(never)]
fn escape(cx: i64, cy: i64, maxiter: i64) -> i64 {
    let (mut zx, mut zy, mut zx2, mut zy2, mut i) = (0i64, 0i64, 0i64, 0i64, 0i64);
    while i < maxiter && zx2 + zy2 <= LIMIT {
        let zxy = (zx * zy) >> 16;
        zx = zx2 - zy2 + cx;
        zy = 2 * zxy + cy;
        zx2 = (zx * zx) >> 16;
        zy2 = (zy * zy) >> 16;
        i += 1;
    }
    i
}

#[inline(never)]
fn grid(width: i64, height: i64, maxiter: i64) -> i64 {
    let mut total = 0i64;
    for py in 0..height {
        let cy = ((py * 163840) / height) - 81920;
        for px in 0..width {
            let cx = ((px * 163840) / width) - 131072;
            total += escape(cx, cy, maxiter);
        }
    }
    total
}

fn main() {
    println!("{}", grid(400, 400, 1000));
}
