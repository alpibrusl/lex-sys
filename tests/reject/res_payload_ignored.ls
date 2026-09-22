//~ ERROR this payload is `res`
//~ RULE linear-value-unconsumed

// The same rule one level down: `_` in a payload position drops whatever was
// there. Fine for an `int`, not for a `Ticket`.

res struct Ticket { fd: int }

enum Slot {
    Empty,
    Full(Ticket),
}

fn size(s: Slot) -> [] int {
    match s {
        Slot::Empty => {
            return 0;
        }
        Slot::Full(_) => {
            return 1;
        }
    }
}

fn main() -> [] int {
    return 0;
}
