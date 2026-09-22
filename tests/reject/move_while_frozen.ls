//~ ERROR frozen by an enclosing `borrow`
//~ RULE borrow-conflict

// §5 rule 1: frozen means not movable and not consumable. `close` takes
// ownership, and ownership is exactly what the borrow suspended.

res struct File { fd: int }

fn close(f: File) -> [] int {
    let File { fd } = f;
    return fd;
}

fn move_frozen(f: File) -> [] int {
    borrow f as &r in {
        let fd = close(f);
    }
    return 0;
}

fn main() -> [] int {
    return 0;
}
