//~ ERROR expected `int`, found `bool`

fn main() -> [] int {
    if 1 == true {
        return 0;
    }
    return 1;
}
