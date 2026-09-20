//~ ERROR write `let`, not `var`

// Destructuring takes a value apart once. There is nothing left to reassign,
// so `var` would be a binding that can never mean what it says.

struct Point {
    x: int,
    y: int,
}

fn main() -> [] int {
    var Point { x, y } = Point { x: 1, y: 2 };
    return x + y;
}
