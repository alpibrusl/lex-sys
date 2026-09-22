//~ ERROR does not fit in `int`
//~ RULE literal-out-of-range

fn main() -> [] int {
    return 9223372036854775808;
}
