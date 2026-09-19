// `T` is rigid inside the body: it is not `int`, even though every
// instantiation so far might be.
//~ ERROR expected `T`, found `int`

fn bad[T](x: T) -> T {
    return x + 1;
}

fn main() -> int {
    return 0;
}
