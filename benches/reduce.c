// Summing a buffer the compiler cannot constant-fold. The unchecked form is
// free to vectorise; the checked form is not, because a trap is observable.
// That difference IS the cost of the guarantee at this optimisation level,
// so it is measured rather than engineered away.
#include <stdio.h>
#include <stdlib.h>

#define N 1000000

__attribute__((noinline))
long run(const long *v, long rounds) {
    long total = 0;
    for (long r = 0; r < rounds; r++) {
        for (long i = 0; i < N; i++) {
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
    long t = run(v, 200);
    printf("%ld\n", t);
    free(v);
    return 0;
}
