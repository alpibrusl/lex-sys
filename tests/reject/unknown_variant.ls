//~ ERROR `Shape` has no variant `Blob`

enum Shape { Circle(int) }

fn main() -> [] int {
    return 0;
}

fn make() -> [] Shape {
    return Shape::Blob(1);
}
