// Precedence, associativity, signed division and remainder, comparisons.
//~ STDOUT 7531101
//~ EXIT 0

fn main() -> int {
    putchar(48 + 1 + 2 * 3);         // precedence: 7
    putchar(48 + (10 - 3 - 2));      // left-associative: 5
    putchar(48 + (-6 / 2 + 6));      // truncating division: 3
    putchar(48 + 7 % 3);             // remainder: 1
    putchar(48 + (2 < 3));           // comparisons are 0 or 1 until M1
    putchar(48 + (3 <= 2));
    putchar(48 + (4 == 4) + (5 != 5));
    putchar(10);
    return 0;
}
