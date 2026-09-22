//~ ERROR must be consumed on every path
//~ RULE linear-value-unconsumed

// §4: every path, not some path. The `else` returns with `f` still live.

res struct Ticket { fd: int }

fn close(f: Ticket) -> [] int {
    let Ticket { fd } = f;
    return fd;
}

fn sometimes(f: Ticket, flag: bool) -> [] int {
    if flag {
        let fd = close(f);
        return 0;
    } else {
        return 1;
    }
}

fn main() -> [] int {
    return 0;
}
