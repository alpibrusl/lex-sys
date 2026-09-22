// Summing a buffer the compiler cannot constant-fold. The unchecked form is
// free to vectorise; the checked form is not, because a trap is observable.
// That difference IS the cost of the guarantee at this optimisation level,
// so it is measured rather than engineered away.
//
// Two guards, switchable independently, because they turn out to cost
// very different things (`docs/gpu.md` §2): -DCHECKED adds the overflow
// trap and -DBOUNDS adds an index check. The second is free and the
// first is not, which is the finding.
#include <stdio.h>
#include <stdlib.h>

#define N 1000000

__attribute__((noinline))
long run(const long *v, long n, long rounds) {
    long total = 0;
    for (long r = 0; r < rounds; r++) {
        for (long i = 0; i < n; i++) {
#if BOUNDS
            if ((unsigned long)i >= (unsigned long)n) __builtin_trap();
#endif
#if CHECKED
            if (__builtin_saddl_overflow(total, v[i], &total)) __builtin_trap();
#else
            total = (long)((unsigned long)total + (unsigned long)v[i]);
#endif
        }
    }
    return total;
}

int main(int argc, char **argv) {
    (void)argv;
    long *v = malloc(N * sizeof(long));
    for (long i = 0; i < N; i++) v[i] = (i % 3) - 1 + (argc - 1);
    long t = run(v, N, 200);
    printf("%ld\n", t);
    free(v);
    return 0;
}
