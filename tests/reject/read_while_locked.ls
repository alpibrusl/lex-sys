//~ ERROR uniquely borrowed here, so nothing else may read it

// §5 rule 2: locked means nothing else may touch `c` at all -- not even a
// read. That is stronger than frozen, and it is what makes `&!` mean unique:
// if the owner could still read the value, the reference would not be the
// only way to reach it, and the writes going through it would be observable
// from two places at once.

struct Counter {
    n: int,
}

fn main() -> [] int {
    var c = Counter { n: 1 };
    borrow mut c as &!r in {
        let peek = c;
    }
    return 0;
}
