//~ ERROR expected `int`, found `bool`

enum Shape { Circle(int) }

fn make() -> Shape {
    return Shape::Circle(true);
}

fn main() -> int {
    return 0;
}
