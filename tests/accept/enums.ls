// Enums with payloads, matched exhaustively, including a struct inside a
// payload and a `_` arm. An enum is a tag plus every variant's payload in M1 --
// overlaying them would be a layout decision, and M1 makes none.
//~ STDOUT 001220069901
//~ EXIT 0

struct Point { x: int, y: int }

enum Shape {
    Empty,
    Circle(int),
    Rect(int, int),
    At(Point, int),
}

fn area(s: Shape) -> [] int {
    match s {
        Shape::Empty => { return 0; }
        Shape::Circle(r) => { return 3 * r * r; }
        Shape::Rect(w, h) => { return w * h; }
        Shape::At(p, r) => { return p.x + p.y + r; }
    }
}

fn describe(s: Shape) -> [] int {
    match s {
        Shape::Circle(_) => { return 99; }
        _ => { return 1; }
    }
}

fn digits(n: int) -> [io] int {
    putchar(48 + n / 10);
    putchar(48 + n % 10);
    return n;
}

fn main() -> [io] int {
    digits(area(Shape::Empty));            // 00
    digits(area(Shape::Circle(2)));        // 12
    digits(area(Shape::Rect(4, 5)));       // 20
    digits(area(Shape::At(Point { x: 1, y: 2 }, 3)));  // 06
    digits(describe(Shape::Circle(1)));    // 99
    digits(describe(Shape::Empty));        // 01
    putchar(10);
    return 0;
}
