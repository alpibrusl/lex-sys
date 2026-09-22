// What every check costs a vectoriser, and whether poison costs less.
//
// `docs/overflow-cost.md` §3.2 found that a check costs the vectoriser
// rather than a branch, measured the overflow one, and generalised.
// `docs/gpu.md` §2.1 falsified half of that -- a bounds check is free --
// and `docs/check-cost.md` falsified the rest: six of the eight checks
// this language emits in a loop body take the SIMD count to zero, and
// what decides it is whether the loop already proves the condition.
//
// This file is one kernel per check, each written so the *unchecked*
// form is as vectorisable as the instruction set allows, so that what
// the check costs is visible rather than hidden behind a loop that was
// scalar anyway.
//
//   -DKERNEL=n   which check (see the table below)
//   -DMODE=m     0 unchecked, 1 the trap this language emits, 2 poison
//
// Mode 2 is `gpu.md` §4.1's option (2), measured rather than assumed:
// the condition is OR-ed into a flag instead of trapping, the operation
// keeps a **defined** result, and the flag is tested once after the
// loop. A flag OR-ed across elements is a reduction, so unlike a trap it
// is reassociable -- which is the hypothesis. What it gives up is
// *where*: the program learns that something went wrong, not which
// element, and it computes with the defined-but-wrong value until the
// boundary.
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
//   6  float_to_int  `truncate(f)`, which traps on NaN and on out of range
//   7  divide        `a / b`, where the trap is the hardware's already
//   8  subslice_iv   the same two tests, on the induction variable
//   9  overflow_each the same overflow check, element-wise not carried
//  10  overflow_signs the same again, as sign logic rather than a builtin
//  11  overflow_carried the sign spelling on the reduction of kernel 0
#include <stdio.h>
#include <stdlib.h>
#include <limits.h>

// The conformance suite builds this with a small `ROUNDS` so that
// checking the modes agree costs a second rather than a minute. The shape
// of the loop is what the measurement is about, and that does not depend
// on how many times the outer one goes round.
#ifndef N
#define N 1000000
#endif
#ifndef ROUNDS
#define ROUNDS 200
#endif

#ifndef KERNEL
#define KERNEL 0
#endif

// `GUARDED` is the older spelling, kept because `reduce.c` and the first
// round of measurements used it.
#ifndef MODE
#ifdef GUARDED
#define MODE GUARDED
#else
#define MODE 0
#endif
#endif

#define MODE_OFF 0
#define MODE_TRAP 1
#define MODE_POISON 2

// The flag, tested once after the loop. `static` rather than local so
// nothing can prove the whole thing dead.
static long poisoned;

#if MODE == MODE_TRAP
#define GUARD(c) do { if (c) __builtin_trap(); } while (0)
#elif MODE == MODE_POISON
#define GUARD(c) do { bad |= (long)(c); } while (0)
#else
#define GUARD(c) do { } while (0)
#endif

#if MODE == MODE_POISON
#define POISON_BEGIN long bad = 0;
#define POISON_END   poisoned |= bad; if (poisoned) __builtin_trap();
#else
#define POISON_BEGIN
#define POISON_END
#endif

// What the operation does when the check would have fired. Mode 1 never
// reaches these -- it has already trapped -- and mode 0 never needs them,
// so they exist so that **mode 2 stays defined**: a masked shift, a
// truncated byte, a wrapped negation, a clamped conversion. Each is one
// instruction and vectorises, which is part of what is being priced.
#if MODE == MODE_POISON
#define SAFE_SHIFT(x, k) ((unsigned long)(x) << ((unsigned long)(k) & 63))
#define SAFE_BYTE(n)     ((unsigned long)(unsigned char)(n))
#define SAFE_DOUBLE(x)   ((x) > 9.2233720368547748e18 ? 9.2233720368547748e18 \
                          : ((x) > -9.2233720368547748e18 ? (x) : -9.2233720368547748e18))
#else
#define SAFE_SHIFT(x, k) ((unsigned long)(x) << (unsigned long)(k))
#define SAFE_BYTE(n)     ((unsigned long)(unsigned char)(n))
#define SAFE_DOUBLE(x)   (x)
#endif

#if KERNEL == 0
// Overflow on a running sum: the reduction `overflow-cost.md` measured,
// repeated here so this file carries its own control. In mode 2 the
// builtin still writes the wrapped result, so poison is wrapping
// arithmetic plus a flag.
__attribute__((noinline))
long run(const long *v, const long *w, long n) {
    POISON_BEGIN
    long total = 0;
    for (long r = 0; r < ROUNDS; r++)
        for (long i = 0; i < n; i++) {
            (void)w;
#if MODE == MODE_TRAP
            if (__builtin_saddl_overflow(total, v[i], &total)) __builtin_trap();
#elif MODE == MODE_POISON
            long sum;
            bad |= __builtin_saddl_overflow(total, v[i], &sum);
            total = (long)((unsigned long)total + (unsigned long)v[i]);
            (void)sum;
#else
            total = (long)((unsigned long)total + (unsigned long)v[i]);
#endif
        }
    POISON_END
    return total;
}

#elif KERNEL == 1
// The index check, which `gpu.md` §2.1 found free. The condition is
// provably true inside a loop the compiler already proved bounded.
__attribute__((noinline))
long run(const long *v, const long *w, long n) {
    POISON_BEGIN
    long total = 0;
    for (long r = 0; r < ROUNDS; r++)
        for (long i = 0; i < n; i++) {
            (void)w;
            GUARD((unsigned long)i >= (unsigned long)n);
            total = (long)((unsigned long)total + (unsigned long)v[i]);
        }
    POISON_END
    return total;
}

#elif KERNEL == 2
// A shift by an amount that comes out of memory. lex-sys emits one
// unsigned comparison against 64 (`defined-behaviour.md` §3) -- a
// constant amount folds it away, so a variable one is where it can cost
// anything at all.
__attribute__((noinline))
long run(const long *v, const long *w, long n) {
    POISON_BEGIN
    long total = 0;
    for (long r = 0; r < ROUNDS; r++)
        for (long i = 0; i < n; i++) {
            GUARD((unsigned long)w[i] >= 64);
            total = (long)((unsigned long)total + SAFE_SHIFT(v[i], w[i]));
        }
    POISON_END
    return total;
}

#elif KERNEL == 3
// `byte_of(n)`: one unsigned comparison against 255, because a negative
// integer read as unsigned is enormous. Narrowing is the operation a
// vectoriser is good at, so this is a fair place to ask.
__attribute__((noinline))
long run(const long *v, const long *w, long n) {
    POISON_BEGIN
    long total = 0;
    for (long r = 0; r < ROUNDS; r++)
        for (long i = 0; i < n; i++) {
            (void)w;
            GUARD((unsigned long)v[i] > 255);
            total = (long)((unsigned long)total + SAFE_BYTE(v[i]));
        }
    POISON_END
    return total;
}

#elif KERNEL == 4
// `s[lo..hi]`: **two** comparisons where indexing has one -- `hi > len`
// and `lo > hi` -- so if a count of comparisons were what mattered this
// is where it would show. It is not: kernel 8 is the same two on the
// induction variable, and it is free.
__attribute__((noinline))
long run(const long *v, const long *w, long n) {
    POISON_BEGIN
    long total = 0;
    for (long r = 0; r < ROUNDS; r++)
        for (long i = 0; i < n; i++) {
            long lo = w[i] & 1;
            long hi = lo + 1;
            GUARD((unsigned long)hi > (unsigned long)n);
            GUARD((unsigned long)lo > (unsigned long)hi);
            total = (long)((unsigned long)total + (unsigned long)v[i]);
        }
    POISON_END
    return total;
}

#elif KERNEL == 5
// Unary minus. There is no negate instruction that cannot overflow:
// `-LONG_MIN` is not representable, so this is `0 - x` checked, which is
// the overflow check again in a different spelling.
__attribute__((noinline))
long run(const long *v, const long *w, long n) {
    POISON_BEGIN
    long total = 0;
    for (long r = 0; r < ROUNDS; r++)
        for (long i = 0; i < n; i++) {
            (void)w;
            GUARD(v[i] == LONG_MIN);
            total = (long)((unsigned long)total - (unsigned long)v[i]);
        }
    POISON_END
    return total;
}

#elif KERNEL == 6
// `truncate(f)`: truncation toward zero, trapping on NaN, on the
// infinities and on anything outside the integer range
// (`floating-point.md` §2).
//
// This kernel is **not** the guard lex-sys emits, and
// `docs/emitted-checks.md` §3 is the reading that says so: Cranelift
// emits `cvttsd2si` and one `cmp $0x1`/`jno`, because the conversion
// answers a sentinel that `rax - 1` overflows on and nothing else, while
// the two comparisons below run on every element. Kept as it is rather
// than rewritten, because what this file measures is what a *vectorising*
// compiler does with a data-dependent trap, and the explicit spelling is
// the one such a compiler would be given. The number it produces is an
// upper bound, and §3 there says so where the number is published.
__attribute__((noinline))
long run(const long *v, const long *w, long n) {
    POISON_BEGIN
    const double *f = (const double *)w;
    long total = 0;
    for (long r = 0; r < ROUNDS; r++)
        for (long i = 0; i < n; i++) {
            (void)v;
            double x = f[i];
            GUARD(!(x >= -9.2233720368547758e18 && x <= 9.2233720368547758e18));
            total = (long)((unsigned long)total + (unsigned long)(long)SAFE_DOUBLE(x));
        }
    POISON_END
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
    POISON_BEGIN
    long total = 0;
    for (long r = 0; r < ROUNDS; r++)
        for (long i = 0; i < n; i++) {
            (void)w;
            long lo = i;
            long hi = i + 1;
            GUARD((unsigned long)hi > (unsigned long)n);
            GUARD((unsigned long)lo > (unsigned long)hi);
            total = (long)((unsigned long)total + (unsigned long)v[i]);
        }
    POISON_END
    return total;
}

#elif KERNEL == 9
// Overflow again, on an **element-wise** addition rather than on the
// running sum. This is the control that separates two things kernel 0
// runs together: whether poison fails on *arithmetic*, or on a condition
// the reduction itself carries.
//
// In kernel 0 the question "does `total + v[i]` overflow" depends on
// `total`, so it is as serial as the sum is and there is no per-lane
// flag to compute. Here the question is about `v[i] + w[i]` and nothing
// else, so if poison works at all it works here.
__attribute__((noinline))
long run(const long *v, const long *w, long n) {
    POISON_BEGIN
    long total = 0;
    for (long r = 0; r < ROUNDS; r++)
        for (long i = 0; i < n; i++) {
            long each;
#if MODE == MODE_TRAP
            if (__builtin_saddl_overflow(v[i], w[i], &each)) __builtin_trap();
#elif MODE == MODE_POISON
            bad |= __builtin_saddl_overflow(v[i], w[i], &each);
#else
            each = (long)((unsigned long)v[i] + (unsigned long)w[i]);
#endif
            total = (long)((unsigned long)total + (unsigned long)each);
        }
    POISON_END
    return total;
}

#elif KERNEL == 10
// The same element-wise addition again, with the overflow condition
// written as **sign logic** instead of as `__builtin_saddl_overflow`.
//
// Kernel 9 says poison does not rescue the overflow check. This asks
// whether that is a fact about overflow or about the *builtin*: a signed
// add overflows exactly when both operands differ in sign from the
// result, which is three XORs, an AND and a compare -- every one of them
// an operation a vectoriser has. If this vectorises and kernel 9 does
// not, then the barrier is the spelling rather than the semantics, and a
// backend that emits its own IR can choose the other spelling.
__attribute__((noinline))
long run(const long *v, const long *w, long n) {
    POISON_BEGIN
    long total = 0;
    for (long r = 0; r < ROUNDS; r++)
        for (long i = 0; i < n; i++) {
            long each;
#if MODE == MODE_TRAP
            if (__builtin_saddl_overflow(v[i], w[i], &each)) __builtin_trap();
#elif MODE == MODE_POISON
            each = (long)((unsigned long)v[i] + (unsigned long)w[i]);
            bad |= ((v[i] ^ each) & (w[i] ^ each)) < 0;
#else
            each = (long)((unsigned long)v[i] + (unsigned long)w[i]);
#endif
            total = (long)((unsigned long)total + (unsigned long)each);
        }
    POISON_END
    return total;
}

#elif KERNEL == 11
// The sign spelling applied to the **reduction** of kernel 0, which is
// the last thing that could rescue it.
//
// Kernel 10 shows the sign test vectorises where the builtin does not.
// If that were the whole story this kernel would vectorise too. If it
// does not, the reduction case is structural rather than a spelling
// problem -- "no partial sum overflowed" is a claim about *this*
// association order, and four lanes compute four different partial sums,
// so the property is not the same property after reassociation.
__attribute__((noinline))
long run(const long *v, const long *w, long n) {
    POISON_BEGIN
    long total = 0;
    for (long r = 0; r < ROUNDS; r++)
        for (long i = 0; i < n; i++) {
            (void)w;
#if MODE == MODE_TRAP
            if (__builtin_saddl_overflow(total, v[i], &total)) __builtin_trap();
#elif MODE == MODE_POISON
            long sum = (long)((unsigned long)total + (unsigned long)v[i]);
            bad |= ((total ^ sum) & (v[i] ^ sum)) < 0;
            total = sum;
#else
            total = (long)((unsigned long)total + (unsigned long)v[i]);
#endif
        }
    POISON_END
    return total;
}

#else
// Division. All three modes are the same program, which is the finding
// rather than a gap in the harness: integer division has no packed form
// on any of these instruction sets, so there is nothing for a guard to
// cost.
//
// This comment used to say "lex-sys emits no comparison at all here: the
// hardware faults on a zero divisor and on `LONG_MIN / -1`". It emits
// one -- `test %rsi,%rsi` and a branch to its own `ud2`, so a zero
// divisor is SIGILL rather than SIGFPE -- and `%` emits a different one
// again. `docs/emitted-checks.md` §4 reads both out of the binary. The
// 1.00x is unaffected: one predictable compare is nothing beside a
// 40-cycle `idiv`.
__attribute__((noinline))
long run(const long *v, const long *w, long n) {
    POISON_BEGIN
    long total = 0;
    for (long r = 0; r < ROUNDS; r++)
        for (long i = 0; i < n; i++)
            total = (long)((unsigned long)total + (unsigned long)(v[i] / w[i]));
    POISON_END
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
        // In range for a byte, because the check is the point and a value
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
