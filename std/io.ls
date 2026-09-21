module std.io;

import std.math;

// `std.io` — the console.
//
// Every function here takes an `&!i Io` and declares `[io_write]`,
// because a library does not get to be quieter about its effects than a
// program would be. That is `docs/modules.md` §6 in practice: `pub`
// bought these functions reachability and nothing else. A caller reading
// `[io_write]` on `print_nat` learns the same thing it would learn from
// a `print_nat` in its own file, which is the point.

pub fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io_write] int {
    var n = 0;
    while n < len(s) {
        putchar(io, int_of(s[n]));
        n = n + 1;
    }
    return len(s);
}

// A non-negative integer, decimal, most significant digit first.
//
// Recursive because the digits come out backwards otherwise, and the
// language has no buffer to reverse them in without a `Heap` this
// function does not take.
pub fn print_nat[&i](io: &!i Io, n: int) -> [io_write] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, 48 + n % 10);
}

// Any integer, with the sign.
//
// The negative case cannot be `print_nat(io, abs(n))`: `abs` traps on
// the most negative integer (`std.math`), and a printer that dies on one
// value in its range would be worse than useless. So the digits come out
// of the negative number directly, which needs no negation anywhere --
// `n % 10` is negative here and `0 - (n % 10)` is a single digit, which
// never overflows.
pub fn print_int[&i](io: &!i Io, n: int) -> [io_write] int {
    if n >= 0 {
        return print_nat(io, n);
    }
    putchar(io, 45);
    return print_negative(io, n);
}

// The digits of a negative number, without ever negating it.
fn print_negative[&i](io: &!i Io, n: int) -> [io_write] int {
    if n <= 0 - 10 {
        print_negative(io, n / 10);
    }
    return putchar(io, 48 + (0 - (n % 10)));
}

// How many characters `print_int` would write.
pub fn width(n: int) -> [] int {
    var digits = 1;
    if n < 0 {
        digits = 2;
    }
    var rest = n / 10;
    while rest != 0 {
        digits = digits + 1;
        rest = rest / 10;
    }
    return digits;
}

// Right-aligned in a field of `field` characters, for columns.
//
// Wider than the field means no padding rather than truncation: a
// number cut in half is the silently wrong answer, and a column that
// grows is merely untidy.
pub fn print_pad[&i](io: &!i Io, n: int, field: int) -> [io_write] int {
    var pad = math.max(0, field - width(n));
    while pad > 0 {
        putchar(io, 32);
        pad = pad - 1;
    }
    return print_int(io, n);
}

pub fn newline[&i](io: &!i Io) -> [io_write] int {
    return putchar(io, 10);
}

pub fn space[&i](io: &!i Io) -> [io_write] int {
    return putchar(io, 32);
}
