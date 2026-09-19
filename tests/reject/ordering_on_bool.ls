//~ ERROR expected `int`, found `bool`

fn main() -> int {
    if true < false {
        return 0;
    }
    return 1;
}
