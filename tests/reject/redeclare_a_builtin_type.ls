//~ ERROR `int` is a built-in type and cannot be redeclared
//~ RULE builtin-redeclared

struct int { x: int }

fn main() -> [] int {
    return 0;
}
