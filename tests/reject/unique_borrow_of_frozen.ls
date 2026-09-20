//~ ERROR cannot be borrowed uniquely here

// The other direction of the same rule. A shared borrow promises the value
// will not change; taking a unique one inside it would be promising to
// change it. Shared borrows nest with each other and with nothing else.

struct Counter {
    n: int,
}

fn main() -> [] int {
    var c = Counter { n: 1 };
    borrow c as &shared in {
        borrow mut c as &!unique in {
            return 0;
        }
    }
}
