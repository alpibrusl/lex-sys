//~ ERROR is an enum; its payload is read by matching on it
//~ RULE match-on-a-non-enum

enum Shape { Circle(int) }

fn main() -> [] int {
    let s = Shape::Circle(1);
    return s.x;
}
