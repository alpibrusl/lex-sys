//~ ERROR would hold a reference into `r`

// The second way out of a region, and the reason §5 rule 4 is checked over
// every binding rather than only over `return`.
//
// `hole`'s type is still a hole when the block opens. The annotation inside
// fills it with `&r Bytes`, which would leave a binding declared outside the
// block holding a reference into it -- an escape that no `return` statement
// was ever involved in.

struct Bytes {
    len: int,
}

enum Holder[T] {
    Empty,
    Full(T),
}

fn main() -> int {
    let b = Bytes { len: 1 };
    let hole = Holder::Empty;
    borrow b as &r in {
        let used: Holder[&r Bytes] = hole;
    }
    return 0;
}
