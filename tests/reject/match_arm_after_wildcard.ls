//~ ERROR unreachable

enum Shape { Circle(int), Rect(int, int) }

fn main() -> int {
    match Shape::Circle(1) {
        _ => { return 0; }
        Shape::Rect(w, h) => { return w + h; }
    }
}
