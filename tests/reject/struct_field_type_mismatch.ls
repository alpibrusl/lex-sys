//~ ERROR expected `int`, found `bool`

struct Point { x: int, y: int }

fn main() -> [] int {
    let p = Point { x: true, y: 2 };
    return p.y;
}
