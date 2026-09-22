//~ ERROR field `x` is declared twice in `Point`
//~ RULE duplicate-declaration

struct Point { x: int, x: bool }

fn main() -> [] int {
    return 0;
}
