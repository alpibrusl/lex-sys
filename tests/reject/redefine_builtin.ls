//~ ERROR builtin
//~ RULE builtin-redeclared

fn putchar(c: int) -> [] int {
    return c;
}

fn main() -> [] int {
    return 0;
}
