// Nothing about the call says what `T` is: it appears only in the return type.
//~ ERROR cannot tell what `T` is in this call to `make`

enum Option[T] { None, Some(T) }

fn make[T]() -> Option[T] {
    return Option::None;
}

fn main() -> int {
    let x = make();
    return 0;
}
