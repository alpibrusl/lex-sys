//~ ERROR assign the whole value instead

// A place is a whole binding or a field reached through a unique reference,
// and deliberately not a field of an owned local. That would be a partial
// write, and what a partial write means for a binding holding a `res` field
// is a question §4 does not answer -- assigning the whole value says the
// same thing and asks nothing new.

struct Counter {
    n: int,
}

fn main() -> [] int {
    var c = Counter { n: 1 };
    c.n = 2;
    return 0;
}
