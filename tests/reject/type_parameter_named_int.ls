//~ ERROR `int` is a built-in type and cannot be a type parameter
//~ RULE builtin-redeclared

fn f[int](x: int) -> [] int {
    return x;
}

fn main() -> [] int {
    return 0;
}
