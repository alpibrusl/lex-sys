//~ ERROR expected `bool`, found `int`
//~ RULE type-mismatch

fn main() -> [] int {
    if 1 && 2 {
        return 0;
    }
    return 1;
}
