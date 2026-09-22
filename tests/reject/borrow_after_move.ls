//~ ERROR nothing left to borrow
//~ RULE linear-use-after-move

// A borrow needs something to point at. Once `close` has taken the value
// there is no referent, so this is caught before any region reasoning.

res struct Ticket { fd: int }

fn close(f: Ticket) -> [] int {
    let Ticket { fd } = f;
    return fd;
}

fn late(f: Ticket) -> [] int {
    let fd = close(f);
    borrow f as &r in {
        return fd;
    }
}

fn main() -> [] int {
    return 0;
}
