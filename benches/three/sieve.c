/* The same algorithm as `benches/sieve_checked.ls`, in C.
 *
 * Memory-bound where `mandelbrot` is compute-bound, so the two together
 * say whether the gap in `docs/against-c-and-rust.md` §2 is about
 * arithmetic or about everything.
 *
 * Trapping semantics, to match lex-sys: the comparison is only a
 * language comparison when both sides promise the same thing. */
#include <stdio.h>
#include <stdlib.h>

#define LIMIT 60000
#define ROUNDS 1000

static long add(long a, long b) {
    long r;
    if (__builtin_saddl_overflow(a, b, &r)) __builtin_trap();
    return r;
}
static long mul(long a, long b) {
    long r;
    if (__builtin_smull_overflow(a, b, &r)) __builtin_trap();
    return r;
}

__attribute__((noinline))
static long run(void) {
    unsigned char *mark = malloc(LIMIT);
    long found = 0;
    for (long r = 0; r < ROUNDS; r++) {
        for (long i = 0; i < LIMIT; i++) mark[i] = 0;
        for (long p = 2; mul(p, p) < LIMIT; p = add(p, 1)) {
            if (!mark[p]) {
                for (long m = mul(p, p); m < LIMIT; m = add(m, p)) mark[m] = 1;
            }
        }
        found = 0;
        for (long i = 2; i < LIMIT; i++) {
            if (!mark[i]) found = add(found, 1);
        }
    }
    free(mark);
    return found;
}

int main(void) {
    printf("%ld\n", run());
    return 0;
}
