//~ ERROR performs `io`, which its row [] does not declare

// §7.2: the check at a call site is that the callee's row is a subset of the
// enclosing function's declared row. `putchar` performs `io` and this row is
// empty, so the signature is a lie about what calling it costs.
//
// The fix the message names is deliberate in that order: narrow the *body*,
// not the signature. Widening the row is always available and always the
// second choice.

fn quiet() -> [] int {
    putchar(65);
    return 0;
}

fn main() -> [] int {
    return 0;
}
