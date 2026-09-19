//~ ERROR type parameter `T` is declared twice

fn f[T, T](x: T) -> T {
    return x;
}

fn main() -> int {
    return 0;
}
