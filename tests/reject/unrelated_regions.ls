//~ ERROR does not outlive

// §5.2: two references from different `borrow` blocks have different regions.
// `same` declares that both its arguments share one region, and neither of
// these two outlives the other, so there is no region to pick.
//
// Rust reaches for subtyping and variance here. The stack does it instead:
// `b`'s block does not enclose `a`'s, and that is the whole answer.

res struct File { fd: int }

fn close(f: File) -> int {
    let File { fd } = f;
    return fd;
}

fn same[&p](a: &p File, b: &p File) -> int {
    return 0;
}

fn unrelated(x: File, y: File) -> int {
    borrow x as &a in {
        borrow y as &b in {
            let n = same(a, b);
        }
    }
    return close(x) + close(y);
}

fn main() -> int {
    return 0;
}
