//~ ERROR expected `int`, found `bool`

fn main() -> int {
    var x = 1;
    x = true;
    return x;
}
