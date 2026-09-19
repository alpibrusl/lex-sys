//~ ERROR `borrow mut` is not implemented yet

// The syntax is settled and the checker is not. Refusing with the reason is
// better than accepting a `&!r` this slice cannot enforce anything about --
// a unique borrow whose uniqueness nobody checks is worse than none.

struct Counter { n: int }

fn main() -> int {
    let c = Counter { n: 1 };
    borrow mut c as &!r in {
        return 0;
    }
    return 0;
}
