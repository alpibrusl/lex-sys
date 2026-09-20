//~ ERROR `w` is bound twice in this pattern

enum Shape { Rect(int, int) }

fn main() -> [] int {
    match Shape::Rect(1, 2) {
        Shape::Rect(w, w) => { return w; }
    }
}
