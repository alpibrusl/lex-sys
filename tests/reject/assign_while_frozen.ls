//~ ERROR borrowed by an enclosing `borrow`
//~ RULE borrow-conflict

// The third way to break the promise a shared borrow makes. §5 rule 1 names
// moving and consuming; assignment changes the value underneath a reference
// to it, which is the same betrayal by a different route.

struct Counter { n: int }

fn main() -> [] int {
    var c = Counter { n: 1 };
    borrow c as &r in {
        c = Counter { n: 2 };
    }
    return 0;
}
