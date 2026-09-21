/* PROMISED=1 declares `slow_pure` pure with `__attribute__((const))`.
 *
 * The attribute is an **unchecked promise**: write it on a function that
 * is not pure and the program miscompiles silently, with no diagnostic
 * ever. That is the difference `docs/purity.md` §1 is about -- lex-sys
 * computes the same fact and the type checker proves it. */
#include <stdio.h>

#if PROMISED
__attribute__((const))
#endif
long slow_pure(long x);

__attribute__((noinline))
static long run(long rounds) {
    long total = 0;
    for (long i = 0; i < rounds; i++) {
        total = (long)((unsigned long)total
                       + (unsigned long)slow_pure(7) + (unsigned long)slow_pure(7));
    }
    return total;
}

int main(void) {
    printf("%ld\n", run(2000000));
    return 0;
}
