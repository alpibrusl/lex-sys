//~ ERROR does not outlive
//~ RULE reference-escapes-region

// §5.2: two references from different `borrow` blocks have different regions.
// `same` declares that both its arguments share one region, and neither of
// these two outlives the other, so there is no region to pick.
//
// Rust reaches for subtyping and variance here. The stack does it instead:
// `b`'s block does not enclose `a`'s, and that is the whole answer.

res struct Ticket { fd: int }

fn close(f: Ticket) -> [] int {
    let Ticket { fd } = f;
    return fd;
}

fn same[&p](a: &p Ticket, b: &p Ticket) -> [] int {
    return 0;
}

fn unrelated(x: Ticket, y: Ticket) -> [] int {
    borrow x as &a in {
        borrow y as &b in {
            let n = same(a, b);
        }
    }
    return close(x) + close(y);
}

fn main() -> [] int {
    return 0;
}
