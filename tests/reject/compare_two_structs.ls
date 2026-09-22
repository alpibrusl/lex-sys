//~ ERROR cannot be compared with `==`
//~ RULE operator-type-mismatch

struct Point { x: int, y: int }

fn main() -> [] int {
    let a = Point { x: 1, y: 2 };
    let b = Point { x: 1, y: 2 };
    if a == b {
        return 0;
    }
    return 1;
}
