//~ ERROR shared reference `&`, which promises its referent will not change

// A shared reference is a read. Writing through one would break the promise
// its region is built on -- and that promise is exactly why `&r T` is `val`
// and why the backend needs no write-back when the block closes.

struct Counter {
    n: int,
}

fn main() -> [] int {
    var c = Counter { n: 1 };
    borrow c as &r in {
        r.n = 2;
    }
    return 0;
}
