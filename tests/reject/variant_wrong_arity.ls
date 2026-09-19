//~ ERROR carries 2 values, but 1 was given

enum Shape { Rect(int, int) }

fn make() -> Shape {
    return Shape::Rect(1);
}

fn main() -> int {
    return 0;
}
