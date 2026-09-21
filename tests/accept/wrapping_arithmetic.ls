// `docs/defined-behaviour.md` §2: `+`, `-` and `*` are checked and trap on
// overflow. Wraparound is still expressible, but it has to be *asked for*.
//
// The asymmetry is the whole design. `+` is what you write when you mean
// arithmetic; `wrapping_add` is what you write when you mean the bits. You
// cannot get the second by accident, which is the difference between a
// checksum that wraps on purpose and a balance that wraps by mistake.
//
// That overflow traps rather than wrapping is checked by a conformance test
// rather than here: the program dies, so it has no output to declare.
//~ STDOUT wrapped: 1 1 1
//~ STDOUT checked: 6
//~ EXIT 0

fn space[&i](io: &!i Io) -> [io_write] int {
    return putchar(io, 32);
}

fn print_nat[&i](io: &!i Io, n: int) -> [io_write] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, 48 + n % 10);
}

fn digit[&i](io: &!i Io, b: bool) -> [io_write] int {
    if b {
        return putchar(io, 49);
    }
    return putchar(io, 48);
}

fn highest() -> [] int {
    return 9223372036854775807;
}

// Written as a subtraction because `-9223372036854775808` is a literal the
// parser accepts only where a negation is; this is the same value, and
// checked arithmetic is happy with it because the result fits.
fn lowest() -> [] int {
    return 0 - 9223372036854775807 - 1;
}

fn run[&i](io: &!i Io) -> [io_write] int {
    putchar(io, 119); putchar(io, 114); putchar(io, 97); putchar(io, 112);
    putchar(io, 112); putchar(io, 101); putchar(io, 100); putchar(io, 58);
    space(io);                                                  // "wrapped: "

    // One past the top is the bottom.
    digit(io, wrapping_add(highest(), 1) == lowest());
    // One below the bottom is the top.
    space(io); digit(io, wrapping_sub(lowest(), 1) == highest());
    // And multiplication wraps the same way: 2 * (2^63 - 1) is 2^64 - 2,
    // which is -2 read as a signed 64-bit integer.
    space(io); digit(io, wrapping_mul(highest(), 2) == 0 - 2);
    putchar(io, 10);

    putchar(io, 99); putchar(io, 104); putchar(io, 101); putchar(io, 99);
    putchar(io, 107); putchar(io, 101); putchar(io, 100); putchar(io, 58);
    space(io);                                                  // "checked: "
    // Ordinary arithmetic, which is checked, and which is right.
    print_nat(io, 2 + 4);
    return putchar(io, 10);
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // This program touches no files, so that authority ends here.
    release(heap);
    release(fs);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    var status = 0;
    borrow mut io as &!i in {
        status = run(i);
    }
    release(io);
    return status - 10;
}
