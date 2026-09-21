// The same picture in double precision -- the row lex-sys cannot fill.
// `docs/against-c-and-rust.md` §4.

#[inline(never)]
fn escape(cx: f64, cy: f64, maxiter: i64) -> i64 {
    let (mut zx, mut zy, mut zx2, mut zy2) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut i = 0i64;
    while i < maxiter && zx2 + zy2 <= 4.0 {
        let zxy = zx * zy;
        zx = zx2 - zy2 + cx;
        zy = 2.0 * zxy + cy;
        zx2 = zx * zx;
        zy2 = zy * zy;
        i += 1;
    }
    i
}

#[inline(never)]
fn grid(width: i64, height: i64, maxiter: i64) -> i64 {
    let mut total = 0i64;
    for py in 0..height {
        let cy = (py as f64) * 2.5 / (height as f64) - 1.25;
        for px in 0..width {
            let cx = (px as f64) * 2.5 / (width as f64) - 2.0;
            total += escape(cx, cy, maxiter);
        }
    }
    total
}

fn main() {
    println!("{}", grid(400, 400, 1000));
}
