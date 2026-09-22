// What every *other* check costs a vectoriser.
//
// `docs/overflow-cost.md` §3.2 found that a check costs the vectoriser
// rather than a branch, measured the overflow one, and generalised.
// `docs/gpu.md` §2.1 falsified half of that generalisation -- a bounds
// check is free -- and left the rest as an open row: whether any *other*
// check this language emits is also free is unmeasured.
//
// This is that measurement. One kernel per check lex-sys emits inside a
// loop body, each written so the *unguarded* form is as vectorisable as
// the instruction set allows, so that what the guard costs is visible
// rather than hidden behind a loop that was scalar anyway.
//
//   -DKERNEL=n   which check (see the table below)
//   -DGUARDED=1  emit the trap this language emits
//
// Built and counted by `scripts/guards.py`, which reads the SIMD count
// out of the emitted `run` rather than inferring it from the clock --
// the method `gpu.md` §2 used and did not ship a script for.
//
//   0  overflow      the control: known 10 SIMD -> 0 at -O2
//   1  bounds        the other control: known free
//   2  shift         `a << b` with a data-dependent amount
//   3  byte_of       narrowing an int to a byte
//   4  subslice      `s[lo..hi]` -- two comparisons, not one
//   5  negate        unary minus, which is `0 - x` and can overflow
//   6  float_to_int  `int_of(f)`, which traps on NaN and on out of range
//   7  divide        `a / b`, where the trap is the hardware's already
//   8  subslice_iv   the same two tests, on the induction variable
#include <stdio.h>
#include <stdlib.h>
#include <limits.h>

// The conformance suite builds this with a small `ROUNDS` so that
// checking the two halves agree costs a second rather than a minute. The
// shape of the loop is what the measurement is about, and that does not
// depend on how many times the outer one goes round.
#ifndef N
#define N 1000000
#endif
#ifndef ROUNDS
#define ROUNDS 200
#endif

#ifndef KERNEL
#define KERNEL 0
#endif
#ifndef GUARDED
#define GUARDED 0
#endif

#if GUARDED
#define TRAP_IF(c) do { if (c) __builtin_trap(); } while (0)
#else
#define TRAP_IF(c) do { } while (0)
#endif

#if KERNEL == 0
// Overflow on a running sum: the reduction `overflow-cost.md` measured,
// repeated here so this file carries its own control.
__attribute__((noinline))
long run(const long *v, const long *w, long n) {
    long total = 0;
    for (long r = 0; r < ROUNDS; r++)
        for (long i = 0; i < n; i++) {
            (void)w;
#if GUARDED
            if (__builtin_saddl_overflow(total, v[i], &total)) __builtin_trap();
#else
            total = (long)((unsigned long)total + (unsigned long)v[i]);
#endif
        }
    return total;
}

#elif KERNEL == 1
// The index check, which `gpu.md` §2.1 found free. The condition is
// provably true inside a loop the compiler already proved bounded.
__attribute__((noinline))
long run(const long *v, const long *w, long n) {
    long total = 0;
    for (long r = 0; r < ROUNDS; r++)
        for (long i = 0; i < n; i++) {
            (void)w;
            TRAP_IF((unsigned long)i >= (unsigned long)n);
            total = (long)((unsigned long)total + (unsigned long)v[i]);
        }
    return total;
}

#elif KERNEL == 2
// A shift by an amount that comes out of memory. lex-sys emits one
// unsigned comparison against 64 (`defined-behaviour.md` §3) -- a
// constant amount folds it away, so a variable one is where it can cost
// anything at all.
__attribute__((noinline))
long run(const long *v, const long *w, long n) {
    long total = 0;
    for (long r = 0; r < ROUNDS; r++)
        for (long i = 0; i < n; i++) {
            TRAP_IF((unsigned long)w[i] >= 64);
            total = (long)((unsigned long)total
                           + ((unsigned long)v[i] << (unsigned long)w[i]));
        }
    return total;
}

#elif KERNEL == 3
// `byte_of(n)`: one unsigned comparison against 255, because a negative
// integer read as unsigned is enormous. Narrowing is the operation a
// vectoriser is good at, so this is a fair place to ask.
__attribute__((noinline))
long run(const long *v, const long *w, long n) {
    long total = 0;
    for (long r = 0; r < ROUNDS; r++)
        for (long i = 0; i < n; i++) {
            (void)w;
            TRAP_IF((unsigned long)v[i] > 255);
            total = (long)((unsigned long)total + (unsigned long)(unsigned char)v[i]);
        }
    return total;
}

#elif KERNEL == 4
// `s[lo..hi]`: **two** comparisons where indexing has one -- `hi > len`
// and `lo > hi` -- so if a count of comparisons were what mattered this
// is where it would show.
__attribute__((noinline))
long run(const long *v, const long *w, long n) {
    long total = 0;
    for (long r = 0; r < ROUNDS; r++)
        for (long i = 0; i < n; i++) {
            long lo = w[i] & 1;
            long hi = lo + 1;
            TRAP_IF((unsigned long)hi > (unsigned long)n);
            TRAP_IF((unsigned long)lo > (unsigned long)hi);
            total = (long)((unsigned long)total + (unsigned long)v[i]);
        }
    return total;
}

#elif KERNEL == 5
// Unary minus. There is no negate instruction that cannot overflow:
// `-LONG_MIN` is not representable, so this is `0 - x` checked, which is
// the overflow check again in a different spelling.
__attribute__((noinline))
long run(const long *v, const long *w, long n) {
    long total = 0;
    for (long r = 0; r < ROUNDS; r++)
        for (long i = 0; i < n; i++) {
            (void)w;
            TRAP_IF(v[i] == LONG_MIN);
            total = (long)((unsigned long)total - (unsigned long)v[i]);
        }
    return total;
}

#elif KERNEL == 6
// `int_of(f)`: truncation toward zero, trapping on NaN, on the
// infinities and on anything outside the integer range
// (`floating-point.md` §2). Cranelift's trapping `fcvt_to_sint` does the
// test itself; the C spelling makes it explicit.
__attribute__((noinline))
long run(const long *v, const long *w, long n) {
    const double *f = (const double *)w;
    long total = 0;
    for (long r = 0; r < ROUNDS; r++)
        for (long i = 0; i < n; i++) {
            (void)v;
            double x = f[i];
            TRAP_IF(!(x >= -9.2233720368547758e18 && x <= 9.2233720368547758e18));
            total = (long)((unsigned long)total + (unsigned long)(long)x);
        }
    return total;
}

#elif KERNEL == 8
// The same subslice check, on bounds derived from the **induction
// variable** rather than from memory. This is the control that isolates
// the mechanism: same two comparisons, same trap, and the only
// difference is whether the loop the compiler already proved bounded
// also proves these.
__attribute__((noinline))
long run(const long *v, const long *w, long n) {
    long total = 0;
    for (long r = 0; r < ROUNDS; r++)
        for (long i = 0; i < n; i++) {
            (void)w;
            long lo = i;
            long hi = i + 1;
            TRAP_IF((unsigned long)hi > (unsigned long)n);
            TRAP_IF((unsigned long)lo > (unsigned long)hi);
            total = (long)((unsigned long)total + (unsigned long)v[i]);
        }
    return total;
}

#else
// Division. lex-sys emits no comparison at all here: the hardware faults
// on a zero divisor and on `LONG_MIN / -1`, so the guarantee is already
// in the instruction. GUARDED is the same program either way, which is
// the finding rather than a gap in the harness.
__attribute__((noinline))
long run(const long *v, const long *w, long n) {
    long total = 0;
    for (long r = 0; r < ROUNDS; r++)
        for (long i = 0; i < n; i++)
            total = (long)((unsigned long)total + (unsigned long)(v[i] / w[i]));
    return total;
}
#endif

int main(int argc, char **argv) {
    (void)argv;
    long *v = malloc(N * sizeof(long));
    long *w = malloc(N * sizeof(long));
    double *f = (double *)w;
    for (long i = 0; i < N; i++) {
#if KERNEL == 3
        // In range for a byte, because the guard is the point and a value
        // that trips it would measure the trap rather than the check.
        v[i] = (i % 256) + (argc - 1);
#else
        v[i] = (i % 3) - 1 + (argc - 1);
#endif
#if KERNEL == 6
        f[i] = (double)((i % 101) - 50);
#elif KERNEL == 2
        w[i] = i % 8;
#elif KERNEL == 7
        w[i] = (i % 7) + 1;
#else
        w[i] = (i % 5) + 1;
#endif
    }
    (void)f;
    printf("%ld\n", run(v, w, N));
    free(v);
    free(w);
    return 0;
}
