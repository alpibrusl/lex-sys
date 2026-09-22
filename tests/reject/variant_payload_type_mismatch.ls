//~ ERROR expected `int`, found `bool`
//~ RULE type-mismatch

enum Shape { Circle(int) }

fn make() -> [] Shape {
    return Shape::Circle(true);
}

fn main() -> [] int {
    return 0;
}
