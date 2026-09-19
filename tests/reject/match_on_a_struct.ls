//~ ERROR is a struct, not an enum

struct Point { x: int }

fn main() -> int {
    // Parenthesised, because a bare struct literal here would be taken for
    // the match body -- the same ambiguity `if` has.
    match (Point { x: 1 }) {
        _ => { return 0; }
    }
}
