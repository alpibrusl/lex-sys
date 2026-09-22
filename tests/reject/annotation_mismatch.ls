//~ ERROR expected `int`, found `bool`
//~ RULE type-mismatch

fn main() -> [] int {
    let x: int = true;
    return x;
}
