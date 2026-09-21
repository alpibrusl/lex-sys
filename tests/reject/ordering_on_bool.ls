//~ ERROR `bool` has no ordering (`int` and `float` do)

fn main() -> [] int {
    if true < false {
        return 0;
    }
    return 1;
}
