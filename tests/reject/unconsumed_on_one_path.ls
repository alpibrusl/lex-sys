//~ ERROR must be consumed on every path

// §4: every path, not some path. The `else` returns with `f` still live.

res struct File { fd: int }

fn close(f: File) -> int {
    let File { fd } = f;
    return fd;
}

fn sometimes(f: File, flag: bool) -> int {
    if flag {
        let fd = close(f);
        return 0;
    } else {
        return 1;
    }
}

fn main() -> int {
    return 0;
}
