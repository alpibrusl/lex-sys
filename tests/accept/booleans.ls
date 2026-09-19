// `bool` is a type of its own in M1. A comparison yields one, `if` and `while`
// require one, and there is no conversion in either direction.
//~ STDOUT 1010010
//~ EXIT 0

fn digit(b: bool) -> int {
    if b {
        return 49;                   // '1'
    } else {
        return 48;                   // '0'
    }
}

fn main() -> int {
    putchar(digit(2 < 3));
    putchar(digit(3 <= 2));
    putchar(digit(4 == 4));
    putchar(digit(5 != 5));
    putchar(digit(true && false));
    putchar(digit(true || false));
    putchar(digit(!true));
    putchar(10);
    return 0;
}
