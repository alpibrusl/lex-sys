//~ ERROR type parameter `T` is declared twice
//~ RULE duplicate-declaration

fn f[T, T](x: T) -> [] T {
    return x;
}

fn main() -> [] int {
    return 0;
}
