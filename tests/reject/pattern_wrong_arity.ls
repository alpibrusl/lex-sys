//~ ERROR the pattern binds 1

enum Shape { Rect(int, int) }

fn main() -> [] int {
    match Shape::Rect(1, 2) {
        Shape::Rect(w) => { return w; }
    }
}
