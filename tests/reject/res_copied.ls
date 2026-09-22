//~ ERROR already been consumed
//~ RULE linear-use-after-move

// §3: a `res` value may not be copied. Naming `f` twice asks for two of it.

res struct Ticket { fd: int }

struct Pair {
    a: Ticket,
    b: Ticket,
}

fn twice(f: Ticket) -> [] Pair {
    return Pair { a: f, b: f };
}

fn main() -> [] int {
    return 0;
}
