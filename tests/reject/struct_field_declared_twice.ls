//~ ERROR field `x` is declared twice in `Point`

struct Point { x: int, x: bool }

fn main() -> int {
    return 0;
}
