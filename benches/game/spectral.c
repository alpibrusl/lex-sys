// spectral-norm — the same algorithm as spectral.ls, including its
// hand-written Newton `sqrt` rather than libm's, so that the two
// programs do the same arithmetic. `benchmarks-game.md` §3 records what
// that costs and why it is the honest comparison.
#include <stdio.h>
#include <stdlib.h>

static double eval_a(long i, long j) {
    long sum = i + j;
    return 1.0 / (double)(sum * (sum + 1) / 2 + i + 1);
}

static double sqrt_of(double x) {
    if (x <= 0.0) return 0.0;
    double guess = x > 1.0 ? x / 2.0 : x;
    for (int n = 0; n < 20; n++) guess = (guess + x / guess) / 2.0;
    return guess;
}

static void multiply_av(int n, const double *v, double *out) {
    for (int i = 0; i < n; i++) {
        double sum = 0.0;
        for (int j = 0; j < n; j++) sum += eval_a(i, j) * v[j];
        out[i] = sum;
    }
}

static void multiply_atv(int n, const double *v, double *out) {
    for (int i = 0; i < n; i++) {
        double sum = 0.0;
        for (int j = 0; j < n; j++) sum += eval_a(j, i) * v[j];
        out[i] = sum;
    }
}

static void multiply_atav(int n, const double *v, double *out, double *scratch) {
    multiply_av(n, v, scratch);
    multiply_atv(n, scratch, out);
}

int main(int argc, char **argv) {
    int n = argc > 1 ? atoi(argv[1]) : 100;
    double *u = malloc(n * sizeof(double));
    double *v = malloc(n * sizeof(double));
    double *scratch = malloc(n * sizeof(double));
    for (int i = 0; i < n; i++) { u[i] = 1.0; v[i] = 0.0; scratch[i] = 0.0; }

    for (int round = 0; round < 10; round++) {
        multiply_atav(n, u, v, scratch);
        multiply_atav(n, v, u, scratch);
    }
    double vbv = 0.0, vv = 0.0;
    for (int i = 0; i < n; i++) { vbv += u[i] * v[i]; vv += v[i] * v[i]; }
    printf("%.9f\n", sqrt_of(vbv / vv));
    free(u); free(v); free(scratch);
    return 0;
}
