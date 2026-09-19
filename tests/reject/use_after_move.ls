//~ ERROR already been consumed

// §4.1: `close` took ownership, so there is nothing left to pass again.

res struct File { fd: int }

fn close(f: File) -> int {
    let File { fd } = f;
    return fd;
}

fn after_move(f: File) -> int {
    let first = close(f);
    return close(f);
}

fn main() -> int {
    return 0;
}
