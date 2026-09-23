// fasta -- the same algorithm as fasta.ls, following the Benchmarks
// Game's own specification directly rather than either of its hand-tuned
// C entries: one precomputes a 139968-entry lookup table, the other
// threads across the three records. `benchmarks-game.md` §2 is why
// neither is the comparison are-we-fast-yet's rule asks for.
//
// `double`, not `float`: lex-sys has one floating type, IEEE-754
// binary64 (`docs/floating-point.md`), and this reference has to do the
// same arithmetic as fasta.ls to be a fair comparison. It also turns out
// to reproduce the Benchmarks Game's own published N=1000 output bit for
// bit, which is how the algorithm here was checked before either port
// existed (`fasta-1000.txt`).
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define IM 139968
#define IA 3877
#define IC 29573
#define LINELEN 60

static long seed = 42;

static double next_r(void) {
    seed = (seed * IA + IC) % IM;
    return (double)seed / (double)IM;
}

static const char *alu =
    "GGCCGGGCGCGGTGGCTCACGCCTGTAATCCCAGCACTTTGG"
    "GAGGCCGAGGCGGGCGGATCACCTGAGGTCAGGAGTTCGAGA"
    "CCAGCCTGGCCAACATGGTGAAACCCCGTCTCTACTAAAAAT"
    "ACAAAAATTAGCCGGGCGTGGTGGCGCGCGCCTGTAATCCCA"
    "GCTACTCGGGAGGCTGAGGCAGGAGAATCGCTTGAACCCGGG"
    "AGGCGGAGGTTGCAGTGAGCCGAGATCGCGCCACTGCACTCC"
    "AGCCTGGGCGACAGAGCGAGACTCCGTCTCAAAAA";

// Cycles `seq`, wrapped at LINELEN columns, until `count` bytes are
// written.
static void repeat_fasta(const char *seq, long count) {
    long len = (long)strlen(seq);
    long pos = 0;
    while (count > 0) {
        long line = count < LINELEN ? count : LINELEN;
        for (long i = 0; i < line; i++) putchar(seq[(pos + i) % len]);
        putchar('\n');
        pos = (pos + line) % len;
        count -= line;
    }
}

// Draws `count` symbols, one linear-congruential step and one linear
// search over the cumulative table per byte -- the two things the
// benchmark's description says not to optimise away.
static void random_fasta(const char *symbols, const double *p, int nsym, long count) {
    double cumulative[16];
    double sum = 0.0;
    for (int i = 0; i < nsym; i++) {
        sum += p[i];
        cumulative[i] = sum;
    }
    long col = 0;
    for (long k = 0; k < count; k++) {
        double r = next_r();
        int i = 0;
        while (cumulative[i] < r) i++;
        putchar(symbols[i]);
        col++;
        if (col == LINELEN) {
            putchar('\n');
            col = 0;
        }
    }
    if (col != 0) putchar('\n');
}

int main(int argc, char **argv) {
    long n = argc > 1 ? atol(argv[1]) : 1000;

    static const char *iub = "acgtBDHKMNRSVWY";
    static const double iub_p[] = {
        0.27, 0.12, 0.12, 0.27, 0.02, 0.02, 0.02, 0.02,
        0.02, 0.02, 0.02, 0.02, 0.02, 0.02, 0.02};

    static const char *homosapiens = "acgt";
    static const double homosapiens_p[] = {
        0.3029549426680, 0.1979883004921, 0.1975473066391, 0.3015094502008};

    printf(">ONE Homo sapiens alu\n");
    repeat_fasta(alu, n * 2);
    printf(">TWO IUB ambiguity codes\n");
    random_fasta(iub, iub_p, 15, n * 3);
    printf(">THREE Homo sapiens frequency\n");
    random_fasta(homosapiens, homosapiens_p, 4, n * 5);
    return 0;
}
