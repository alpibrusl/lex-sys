//~ ERROR `Point` is a struct, not an enum

struct Point { x: int }

fn main() -> [] int {
    let p = Point::x(1);
    return 0;
}
