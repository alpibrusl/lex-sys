//~ ERROR `bool` cannot be negated (`int` and `float` can)
//~ RULE operator-type-mismatch

fn main() -> [] int {
    return -true;
}
