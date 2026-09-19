//~ ERROR would discard the `res` value it still holds

// An assignment overwrites. Overwriting a live `res` binding destroys it
// without naming a consumer, so the binding must be spent first.

res struct File { fd: int }

fn open(fd: int) -> File {
    return File { fd: fd };
}

fn close(f: File) -> int {
    let File { fd } = f;
    return fd;
}

fn main() -> int {
    var f = open(1);
    f = open(2);
    return close(f);
}
