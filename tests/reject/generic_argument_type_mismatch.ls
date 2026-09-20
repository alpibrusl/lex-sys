//~ ERROR expected `int`, found `bool`

fn pair_up[T](a: T, b: T) -> [] T {
    return a;
}

fn main() -> [] int {
    return pair_up(1, true);
}
