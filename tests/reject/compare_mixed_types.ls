//~ ERROR expected `int`, found `bool`
//~ RULE type-mismatch

fn main() -> [] int {
    if 1 == true {
        return 0;
    }
    return 1;
}
