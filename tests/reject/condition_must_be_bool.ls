//~ ERROR expected `bool`, found `int`
//~ RULE type-mismatch

fn main() -> [] int {
    if 1 {
        return 0;
    }
    return 1;
}
