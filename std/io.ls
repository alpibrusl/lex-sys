module std.io;

import std.math;

// `std.io` — the console.
//
// Every function here takes an `&!i Io` and declares what it did with
// it -- `[io_write]` for the output stream, `[err_write]` for the
// diagnostic one (`docs/standard-error.md`) -- because a library does
// not get to be quieter about its effects than a program would be. That
// is `docs/modules.md` §6 in practice: `pub` bought these functions
// reachability and nothing else. A caller reading `[io_write]` on
// `print_nat` learns the same thing it would learn from a `print_nat` in
// its own file, which is the point.

// One call, not one per byte (`docs/bulk-io.md`).
//
// This was the `putchar` loop until §1 measured what that costs: eight
// megabytes took 42 ms through it and 3.3 ms through a bulk write, and
// the 12.8× is libc's per-call overhead rather than anything lex-sys
// does — C pays 11× for the same shape. What lex-sys could not do was
// write any other way without asking for `Ffi("libc")`, which §2 is
// about.
pub fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io_write] int {
    return write_bytes(io, s);
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
    return putchar(io, '0' + n % 10);
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
    return putchar(io, '0' + (0 - (n % 10)));
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

// A diagnostic, on the stream a shell redirects with `2>`.
//
// `docs/standard-error.md` §1 is what this is for and §1.2 is why it is
// not `write_all`: standard output is fully buffered when it is not a
// terminal, so a message written just before a trap is never flushed and
// never arrives. Measured at zero bytes, into a file and through a pipe
// both.
//
// Its row is `[err_write]` and not `[io_write]`, so a caller's report
// says which stream it wrote to -- which is the whole reason the label
// is separate (§3.1).
//
// There is no `error_nat` and no `error_int`. A number on this stream
// wants formatting into a buffer and writing once, `std.buffer` already
// does that, and no program here has asked (§3.2).
pub fn error_all[&r, &i](io: &!i Io, s: &r [byte]) -> [err_write] int {
    return write_err(io, s);
}
