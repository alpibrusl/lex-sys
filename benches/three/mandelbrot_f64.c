/* The same picture in double precision -- the row lex-sys cannot fill.
 *
 * Not a fair race against the fixed-point versions and not meant to be.
 * It is here to answer a different question: what does the *absence* of
 * a float type cost, given that a program wanting this picture would
 * write this and not Q16.16. See `docs/against-c-and-rust.md` §4. */
#include <stdio.h>

__attribute__((noinline))
static long escape(double cx, double cy, long maxiter) {
    double zx = 0, zy = 0, zx2 = 0, zy2 = 0;
    long i = 0;
    while (i < maxiter && zx2 + zy2 <= 4.0) {
        double zxy = zx * zy;
        zx = zx2 - zy2 + cx;
        zy = 2 * zxy + cy;
        zx2 = zx * zx;
        zy2 = zy * zy;
        i++;
    }
    return i;
}

__attribute__((noinline))
static long grid(long width, long height, long maxiter) {
    long total = 0;
    for (long py = 0; py < height; py++) {
        double cy = ((double)py * 2.5 / (double)height) - 1.25;
        for (long px = 0; px < width; px++) {
            double cx = ((double)px * 2.5 / (double)width) - 2.0;
            total += escape(cx, cy, maxiter);
        }
    }
    return total;
}

int main(void) {
    printf("%ld\n", grid(400, 400, 1000));
    return 0;
}
