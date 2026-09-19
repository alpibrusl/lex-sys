//~ ERROR `Point` has no field `z`

struct Point { x: int, y: int }

fn main() -> int {
    let p = Point { x: 1, y: 2 };
    return p.z;
}
