//~ ERROR the value matched here is `res`

// §4.1: destructuring consumes by producing the parts. A `_` arm consumes the
// scrutinee and produces nothing, which is a silent drop by another name.

res struct File { fd: int }

enum Slot {
    Empty,
    Full(File),
}

fn size(s: Slot) -> int {
    match s {
        Slot::Empty => {
            return 0;
        }
        _ => {
            return 1;
        }
    }
}

fn main() -> int {
    return 0;
}
