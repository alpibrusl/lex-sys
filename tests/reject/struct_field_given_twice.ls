//~ ERROR field `x` is given twice

struct Point { x: int, y: int }

fn main() -> [] int {
    let p = Point { x: 1, x: 2, y: 3 };
    return p.x;
}
