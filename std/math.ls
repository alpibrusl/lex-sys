module std.math;

// `std.math` — the arithmetic every program writes for itself.

pub fn min(a: int, b: int) -> [] int {
    if a < b {
        return a;
    }
    return b;
}

pub fn max(a: int, b: int) -> [] int {
    if a > b {
        return a;
    }
    return b;
}

// **Traps on the most negative integer**, because negating it
// overflows.
//
// `docs/defined-behaviour.md` §2.1: an operation with no right answer
// stops rather than inventing one. Every other language's `abs` returns
// the negative number here, which is the silently-wrong answer this
// language exists to refuse — so this one does not return at all. The
// trap is `0 - n` doing what `-` already does, not a check added on
// top: the arithmetic is checked, so `abs` is too, for free.
pub fn abs(n: int) -> [] int {
    if n < 0 {
        return 0 - n;
    }
    return n;
}

pub fn sign(n: int) -> [] int {
    if n < 0 {
        return 0 - 1;
    }
    if n > 0 {
        return 1;
    }
    return 0;
}

// Euclid, on magnitudes. `gcd(0, 0)` is 0, which is the convention every
// library uses and the only value that makes `gcd` total.
pub fn gcd(a: int, b: int) -> [] int {
    var x = abs(a);
    var y = abs(b);
    while y != 0 {
        let r = x % y;
        x = y;
        y = r;
    }
    return x;
}
