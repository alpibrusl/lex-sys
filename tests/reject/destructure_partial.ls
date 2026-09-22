//~ ERROR destructuring takes the whole value apart
//~ RULE arity-mismatch

// Naming some of the fields would leave the rest with nowhere to go — a
// partial move, which is the thing §4.1 does not have.

res struct Ticket { fd: int }

struct Pair {
    a: Ticket,
    b: Ticket,
}

fn first(p: Pair) -> [] Ticket {
    let Pair { a } = p;
    return a;
}

fn main() -> [] int {
    return 0;
}
