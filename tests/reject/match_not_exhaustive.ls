//~ ERROR does not cover `Shape::Rect`

enum Shape { Circle(int), Rect(int, int) }

fn area(s: Shape) -> [] int {
    match s {
        Shape::Circle(r) => { return r; }
    }
    return 0;
}

fn main() -> [] int {
    return area(Shape::Circle(1));
}
