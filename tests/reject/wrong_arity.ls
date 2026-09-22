//~ ERROR takes 2 arguments
//~ RULE arity-mismatch

fn add(a: int, b: int) -> [] int {
    return a + b;
}

fn main() -> [] int {
    return add(1);
}
