// `&&` and `||` do not evaluate their right operand when the left already
// decides the answer. `noisy` prints 'X', so the test is that 'X' never
// appears — a fixture that would still pass if the operators were strict is
// not a test of short-circuiting.
//~ STDOUT ab
//~ EXIT 0

fn noisy() -> [io] bool {
    putchar(88);                     // 'X'
    return true;
}

fn main() -> [io] int {
    if false && noisy() {
        putchar(63);                 // '?'
    }
    if true || noisy() {
        putchar(97);                 // 'a'
    }
    putchar(98);                     // 'b'
    putchar(10);
    return 0;
}
