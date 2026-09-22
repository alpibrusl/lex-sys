//~ ERROR frozen by an enclosing `borrow`
//~ RULE borrow-conflict

// §5 rule 1: frozen means not movable and not consumable. `close` takes
// ownership, and ownership is exactly what the borrow suspended.

res struct Ticket { fd: int }

fn close(f: Ticket) -> [] int {
    let Ticket { fd } = f;
    return fd;
}

fn move_frozen(f: Ticket) -> [] int {
    borrow f as &r in {
        let fd = close(f);
    }
    return 0;
}

fn main() -> [] int {
    return 0;
}
