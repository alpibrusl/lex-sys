//~ ERROR `Shape` is an enum, not a struct

enum Shape { Circle(int) }

fn main() -> [] int {
    let s = Shape { x: 1 };
    return 0;
}
