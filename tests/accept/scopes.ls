// An inner block may shadow; the outer binding is untouched.
//~ STDOUT ba
//~ EXIT 0

fn main() -> int {
    let x = 97;
    if 1 {
        let x = 98;
        putchar(x);
    }
    putchar(x);
    putchar(10);
    return 0;
}
