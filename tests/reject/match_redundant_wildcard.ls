//~ ERROR every variant of `Shape` is already matched
//~ RULE match-arm-unreachable

enum Shape { Circle(int) }

fn main() -> [] int {
    match Shape::Circle(1) {
        Shape::Circle(r) => { return r; }
        _ => { return 0; }
    }
}
