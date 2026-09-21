//~ ERROR declares `io_write` but never performs it

// §7.3: an over-wide row is an error, not a warning. The row is exact or it
// is decoration -- and an inexact row means `[]` no longer means pure, which
// costs examples-as-tests and costs a signature hash that means anything.
//
// The price is that a signature cannot reserve room for an implementation
// that has not been written yet. That tension is recorded in §12.

fn pure_after_all() -> [io_write] int {
    return 1;
}

fn main() -> [] int {
    return 0;
}
