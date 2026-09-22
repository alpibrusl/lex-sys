//~ ERROR write `as &!r`
//~ RULE pattern-shape

// `mut` in one place and not the other is a typo, not a shorthand, so neither
// spelling is quietly preferred over the other.

struct Counter { n: int }

fn main() -> [] int {
    let c = Counter { n: 1 };
    borrow mut c as &r in {
        return 0;
    }
    return 0;
}
