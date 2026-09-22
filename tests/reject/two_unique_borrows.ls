//~ ERROR already uniquely borrowed
//~ RULE borrow-conflict

// §5 rule 2: one unique borrow at a time. Two `&!` references to one value
// are two ways to reach it, which is the one thing `&!` promises there are
// not.

struct Counter {
    n: int,
}

fn main() -> [] int {
    var c = Counter { n: 1 };
    borrow mut c as &!a in {
        borrow mut c as &!b in {
            return 0;
        }
    }
}
