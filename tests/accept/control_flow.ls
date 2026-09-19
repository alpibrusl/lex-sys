// Recursion, `while`, `if`/`else if`/`else`, mutable locals.
//~ STDOUT 01123A
//~ EXIT 0

fn fib(n: int) -> int {
    if n < 2 {
        return n;
    } else {
        return fib(n - 1) + fib(n - 2);
    }
}

fn main() -> int {
    var i = 0;
    while i < 5 {
        putchar(48 + fib(i));
        i = i + 1;
    }
    if i == 5 {
        putchar(65);
    } else if i == 4 {
        putchar(66);
    } else {
        putchar(67);
    }
    putchar(10);
    return 0;
}
