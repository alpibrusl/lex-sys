//~ ERROR would discard the `res` value it still holds
//~ RULE linear-value-unconsumed

// An assignment overwrites. Overwriting a live `res` binding destroys it
// without naming a consumer, so the binding must be spent first.

res struct Ticket { fd: int }

fn open(fd: int) -> [] Ticket {
    return Ticket { fd: fd };
}

fn close(f: Ticket) -> [] int {
    let Ticket { fd } = f;
    return fd;
}

fn main() -> [] int {
    var f = open(1);
    f = open(2);
    return close(f);
}
