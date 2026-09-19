//~ ERROR nothing left to borrow

// A borrow needs something to point at. Once `close` has taken the value
// there is no referent, so this is caught before any region reasoning.

res struct File { fd: int }

fn close(f: File) -> int {
    let File { fd } = f;
    return fd;
}

fn late(f: File) -> int {
    let fd = close(f);
    borrow f as &r in {
        return fd;
    }
}

fn main() -> int {
    return 0;
}
