//~ ERROR a reference may not outlive its region
//~ RULE reference-escapes-region

// §5 rule 4: escape is an occurs-check. The block's region is `r`, the
// function's return type can only name `q`, and no `borrow` block outlives a
// region its caller opened -- so the reference has nowhere to go.

res struct Ticket { fd: int }

fn close(f: Ticket) -> [] int {
    let Ticket { fd } = f;
    return fd;
}

fn escape[&q](f: Ticket, fallback: &q Ticket) -> [] &q Ticket {
    borrow f as &r in {
        return r;
    }
    return fallback;
}

fn main() -> [] int {
    return 0;
}
