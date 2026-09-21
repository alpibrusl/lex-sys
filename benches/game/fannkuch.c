// fannkuch-redux — the same algorithm as fannkuch.ls, line for line.
//
// Not the Benchmarks Game's own C entry, which is hand-vectorised and
// threaded: `benchmarks-game.md` §2 is why. This is the comparison
// are-we-fast-yet's rule asks for — same algorithm, same output — and
// the other one would measure a decade of tuning.
#include <stdio.h>
#include <stdlib.h>

static long run(int n, long *maxflips_out) {
    int *perm = malloc(n * sizeof(int));
    int *perm1 = malloc(n * sizeof(int));
    int *count = malloc(n * sizeof(int));
    long checksum = 0, maxflips = 0, permcount = 0;

    for (int i = 0; i < n; i++) perm1[i] = i;
    int r = n;
    for (;;) {
        while (r != 1) { count[r - 1] = r; r--; }
        for (int j = 0; j < n; j++) perm[j] = perm1[j];

        long flips = 0;
        int k = perm[0];
        while (k != 0) {
            int lo = 0, hi = k;
            while (lo < hi) { int s = perm[lo]; perm[lo] = perm[hi]; perm[hi] = s; lo++; hi--; }
            flips++;
            k = perm[0];
        }
        if (flips > maxflips) maxflips = flips;
        checksum += (permcount % 2 == 0) ? flips : -flips;

        for (;;) {
            if (r == n) {
                free(perm); free(perm1); free(count);
                *maxflips_out = maxflips;
                return checksum;
            }
            int first = perm1[0];
            for (int m = 0; m < r; m++) perm1[m] = perm1[m + 1];
            perm1[r] = first;
            if (--count[r] > 0) break;
            r++;
        }
        permcount++;
    }
}

int main(int argc, char **argv) {
    int n = argc > 1 ? atoi(argv[1]) : 7;
    long maxflips = 0;
    long checksum = run(n, &maxflips);
    printf("%ld\nPfannkuchen(%d) = %ld\n", checksum, n, maxflips);
    return 0;
}
