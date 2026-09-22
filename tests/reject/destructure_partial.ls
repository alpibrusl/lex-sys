//~ ERROR destructuring takes the whole value apart
//~ RULE arity-mismatch

// Naming some of the fields would leave the rest with nowhere to go — a
// partial move, which is the thing §4.1 does not have.

res struct File { fd: int }

struct Pair {
    a: File,
    b: File,
}

fn first(p: Pair) -> [] File {
    let Pair { a } = p;
    return a;
}

fn main() -> [] int {
    return 0;
}
