//~ ERROR expected `int`, found `bool`
//~ RULE type-mismatch

fn main() -> [] int {
    var x = 1;
    x = true;
    return x;
}
