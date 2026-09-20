//~ ERROR `Shape::Circle` is matched twice

enum Shape { Circle(int), Rect(int, int) }

fn main() -> [] int {
    match Shape::Circle(1) {
        Shape::Circle(r) => { return r; }
        Shape::Circle(r) => { return r; }
        Shape::Rect(w, h) => { return w + h; }
    }
}
