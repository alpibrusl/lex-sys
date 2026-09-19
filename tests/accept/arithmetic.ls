// Precedence, associativity, signed division and remainder.
//~ STDOUT 7531
//~ EXIT 0

fn main() -> int {
    putchar(48 + 1 + 2 * 3);         // precedence: 7
    putchar(48 + (10 - 3 - 2));      // left-associative: 5
    putchar(48 + (-6 / 2 + 6));      // truncating division: 3
    putchar(48 + 7 % 3);             // remainder: 1
    putchar(10);
    return 0;
}
