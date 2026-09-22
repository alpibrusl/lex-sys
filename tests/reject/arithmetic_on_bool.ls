//~ ERROR `bool` has no arithmetic (`int` and `float` do)
//~ RULE operator-type-mismatch

fn main() -> [] int {
    return true + false;
}
