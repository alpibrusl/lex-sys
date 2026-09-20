//~ ERROR `int` is a built-in type and cannot be redeclared

struct int { x: int }

fn main() -> [] int {
    return 0;
}
