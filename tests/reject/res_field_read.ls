//~ ERROR a field cannot be read out of it
//~ RULE linear-value-taken-apart

// Reading a part out of a `res` value without taking the value apart is a
// non-owning read — a borrow, which is §5 and not in this slice. Until then
// the whole value comes apart at once, or not at all.

res struct Ticket { fd: int }

fn close(f: Ticket) -> [] int {
    let Ticket { fd } = f;
    return fd;
}

fn peek(f: Ticket) -> [] int {
    let n = f.fd;
    return close(f);
}

fn main() -> [] int {
    return 0;
}
