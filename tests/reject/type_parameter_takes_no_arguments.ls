//~ ERROR type parameter `T` takes no type arguments
//~ RULE type-args-not-taken

fn f[T](x: T[int]) -> [] int {
    return 0;
}

fn main() -> [] int {
    return 0;
}
