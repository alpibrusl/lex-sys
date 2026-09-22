//~ ERROR already been consumed
//~ RULE linear-use-after-move

// §4.1: `close` took ownership, so there is nothing left to pass again.

res struct Ticket { fd: int }

fn close(f: Ticket) -> [] int {
    let Ticket { fd } = f;
    return fd;
}

fn after_move(f: Ticket) -> [] int {
    let first = close(f);
    return close(f);
}

fn main() -> [] int {
    return 0;
}
