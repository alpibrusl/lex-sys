#include <stdio.h>
#include <stdlib.h>
int main(void) {
    long n = 4000000;
    long *xs = malloc(n*sizeof(long)), *ys = malloc(n*sizeof(long)), *zs = malloc(n*sizeof(long));
    for (long i = 0; i < n; i++) { xs[i] = 1; ys[i] = 2; zs[i] = 3; }
    long total = 0;
    for (int r = 0; r < 8; r++) for (long i = 0; i < n; i++) total += xs[i];
    printf("%ld\n", total); free(xs); free(ys); free(zs); return 0;
}
