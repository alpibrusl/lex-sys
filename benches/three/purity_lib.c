/* `slow_pure` in its own translation unit, so the caller cannot see the
 * body. That is ordinary separate compilation, and it is where a
 * *declared* purity fact stops being redundant with inlining. */
long slow_pure(long x) {
    long acc = 0;
    for (long i = 0; i < 64; i++) acc = acc * 31 + (x ^ i);
    return acc;
}
