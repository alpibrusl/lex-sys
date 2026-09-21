// `docs/bitwise.md`: all six operators, the precedence table, and the two
// decisions that are not obvious -- `>>` is arithmetic (§2), and a shift
// does not trap on the value it produces (§4).
//
// The shift amounts that *do* trap are not here, because a trapping
// program is one that compiled: the reject harness runs `check`, so they
// are conformance tests instead (`bitwise.md` §7).
//~ STDOUT and 8
//~ STDOUT or 15
//~ STDOUT xor 6
//~ STDOUT not -1
//~ STDOUT shl 16
//~ STDOUT shr -4
//~ STDOUT sign 1
//~ STDOUT mask 13
//~ STDOUT tight 1
//~ EXIT 0

import std.io;

fn label[&i, &s](io: &!i Io, name: &s [byte], value: int) -> [io_write] int {
    io.write_all(io, name);
    io.space(io);
    io.print_int(io, value);
    io.newline(io);
    return 0;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);

    borrow mut io as &!i in {
        label(i, "and", 12 & 10);
        label(i, "or", 12 | 3);
        label(i, "xor", 12 ^ 10);
        // `~0` is every bit set, which read as a signed integer is -1.
        label(i, "not", ~0);
        label(i, "shl", 1 << 4);
        // Arithmetic, so the sign survives: -8 >> 1 is -4 and not a large
        // positive number (§2).
        label(i, "shr", (0 - 8) >> 1);
        // §4: `1 << 63` sets the sign bit and that is the answer, not an
        // overflow. Printed as 1 so the expectation reads as a claim.
        var sign = 0;
        if (1 << 63) < 0 {
            sign = 1;
        }
        label(i, "sign", sign);

        // `strings.md` §4's promise, now writable: the low four bits of a
        // byte, with the range check where `byte_of` puts it.
        let b = byte_of(0xff - 0xf2 + 0xf0);
        label(i, "mask", int_of(b) & 15);

        // §5: bitwise binds tighter than comparison, so this is
        // `(12 & 10) == 8` and not C's `12 & (10 == 8)`.
        var tight = 0;
        if 12 & 10 == 8 {
            tight = 1;
        }
        label(i, "tight", tight);
    }

    release(io);
    return 0;
}
