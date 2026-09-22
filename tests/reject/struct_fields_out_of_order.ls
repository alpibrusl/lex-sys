//~ ERROR declaration order
//~ RULE field-order

// `docs/defined-behaviour.md` §3: the order you read is the order it runs.
//
// A struct's fields are laid out and evaluated in declaration order. Written
// the other way round, this literal would run `second()` before `first()`
// while reading as though it did the opposite -- side effects reordering
// underneath the text, in a language whose whole argument is that effects
// are visible in the text.
//
// So it is refused rather than silently reordered. The fix is to write the
// fields in the order they were declared, which costs nothing and says what
// happens.

struct Pair {
    first: int,
    second: int,
}

fn first() -> [] int {
    return 1;
}

fn second() -> [] int {
    return 2;
}

fn main() -> [] int {
    let p = Pair { second: second(), first: first() };
    return p.first;
}
