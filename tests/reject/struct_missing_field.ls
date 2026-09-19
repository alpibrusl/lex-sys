//~ ERROR missing field `y` in `Point`

struct Point { x: int, y: int }

fn main() -> int {
    let p = Point { x: 1 };
    return p.x;
}
