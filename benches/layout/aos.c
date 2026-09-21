#include <stdio.h>
#include <stdlib.h>
typedef struct { long x, y, z; } P;
int main(void) {
    long n = 4000000; P *a = malloc(n * sizeof(P));
    for (long i = 0; i < n; i++) { a[i].x = 1; a[i].y = 2; a[i].z = 3; }
    long total = 0;
    for (int r = 0; r < 8; r++) for (long i = 0; i < n; i++) total += a[i].x;
    printf("%ld\n", total); free(a); return 0;
}
