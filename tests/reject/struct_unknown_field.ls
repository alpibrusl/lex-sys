//~ ERROR `Point` has no field `z`

struct Point { x: int, y: int }

fn main() -> int {
    let p = Point { x: 1, y: 2, z: 3 };
    return p.x;
}
