/* The same algorithm as `mandelbrot.ls`, in C.
 *
 * CHECKED=1 traps on integer overflow the way lex-sys does, so the two
 * halves bracket what the guarantee costs a mature backend.
 *
 * Note what this file relies on that C does not promise: `>>` on a
 * negative signed value is *implementation-defined* in C, and this code
 * needs it to be arithmetic. gcc and clang both do that; the standard
 * does not say they must. lex-sys defines it (`docs/bitwise.md` §2). */
#include <stdio.h>

#define LIMIT (4 << 16)

#if CHECKED
#include <stdlib.h>
static long mul(long a, long b) {
    long r;
    if (__builtin_smull_overflow(a, b, &r)) __builtin_trap();
    return r;
}
static long add(long a, long b) {
    long r;
    if (__builtin_saddl_overflow(a, b, &r)) __builtin_trap();
    return r;
}
static long sub(long a, long b) {
    long r;
    if (__builtin_ssubl_overflow(a, b, &r)) __builtin_trap();
    return r;
}
#else
static long mul(long a, long b) { return (long)((unsigned long)a * (unsigned long)b); }
static long add(long a, long b) { return (long)((unsigned long)a + (unsigned long)b); }
static long sub(long a, long b) { return (long)((unsigned long)a - (unsigned long)b); }
#endif

__attribute__((noinline))
static long escape(long cx, long cy, long maxiter) {
    long zx = 0, zy = 0, zx2 = 0, zy2 = 0, i = 0;
    while (i < maxiter && add(zx2, zy2) <= LIMIT) {
        long zxy = mul(zx, zy) >> 16;
        zx = add(sub(zx2, zy2), cx);
        zy = add(mul(2, zxy), cy);
        zx2 = mul(zx, zx) >> 16;
        zy2 = mul(zy, zy) >> 16;
        i = add(i, 1);
    }
    return i;
}

__attribute__((noinline))
static long grid(long width, long height, long maxiter) {
    long total = 0;
    for (long py = 0; py < height; py++) {
        long cy = ((py * 163840) / height) - 81920;
        for (long px = 0; px < width; px++) {
            long cx = ((px * 163840) / width) - 131072;
            total = add(total, escape(cx, cy, maxiter));
        }
    }
    return total;
}

int main(void) {
    printf("%ld\n", grid(400, 400, 1000));
    return 0;
}
