// Structs: nested values, passed and returned by value, with field access
// chains. A struct has no layout in M1 -- it is scalarised into its leaf
// fields -- so nothing here depends on how one would be laid out in memory.
//~ STDOUT 18.
//~ EXIT 0

struct Point { x: int, y: int }
struct Line { from: Point, to: Point, dashed: bool }

fn length_squared(l: Line) -> int {
    let dx = l.to.x - l.from.x;
    let dy = l.to.y - l.from.y;
    return dx * dx + dy * dy;
}

fn shift(p: Point, by: int) -> Point {
    return Point { x: p.x + by, y: p.y + by };
}

fn main() -> int {
    let a = Point { x: 1, y: 2 };
    let b = shift(a, 3);
    let l = Line { from: a, to: b, dashed: false };
    // (4-1)^2 + (5-2)^2 = 9 + 9 = 18
    putchar(48 + length_squared(l) / 10);
    putchar(48 + length_squared(l) % 10);
    if l.dashed {
        putchar(33);
    } else {
        putchar(46);
    }
    putchar(10);
    return 0;
}
