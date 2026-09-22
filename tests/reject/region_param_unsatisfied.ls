//~ ERROR requires `src <= dst`
//~ RULE reference-escapes-region

// §5.2: a declared `where` is an obligation the call site discharges with the
// same lexical lookup the body used. Here `dst` is bound to the inner block
// and `src` to the outer one, so the relation runs the wrong way.
//
// Swapping the arguments makes it hold -- see `tests/accept/nested_regions.ls`.

res struct Ticket { fd: int }

fn close(f: Ticket) -> [] int {
    let Ticket { fd } = f;
    return fd;
}

fn copy_into[&dst, &src where src <= dst](d: &dst Ticket, s: &src Ticket) -> [] int {
    return 0;
}

fn unsatisfied(x: Ticket, y: Ticket) -> [] int {
    borrow x as &outer in {
        borrow y as &inner in {
            let n = copy_into(inner, outer);
        }
    }
    return close(x) + close(y);
}

fn main() -> [] int {
    return 0;
}
