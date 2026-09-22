//~ ERROR expected `int`, found `bool`
//~ RULE type-mismatch

struct Point { x: int, y: int }

fn main() -> [] int {
    let p = Point { x: true, y: 2 };
    return p.y;
}
