// `docs/floating-point.md`: literals, arithmetic, comparison, both
// conversions, and the two facts that surprise people.
//
// The trapping conversions are not here, because a trapping program is
// one that compiled: the reject harness runs `check`, so they are
// conformance tests (§8).
//~ STDOUT sum 35
//~ STDOUT half 5
//~ STDOUT neg -27
//~ STDOUT exponent 2
//~ STDOUT nan-is-nan 1
//~ STDOUT nan-equals-itself 0
//~ STDOUT inf-beats-everything 1
//~ STDOUT toward-zero -2
//~ STDOUT bits-of-one 4607182418800017408
//~ STDOUT sign-of-minus-zero 1
//~ STDOUT minus-zero-equals-zero 1
//~ EXIT 0

import std.io;

fn label[&i, &s](io: &!i Io, name: &s [byte], value: int) -> [io_write] int {
    io.write_all(io, name);
    io.space(io);
    io.print_int(io, value);
    io.newline(io);
    return 0;
}

fn flag(b: bool) -> [] int {
    if b {
        return 1;
    }
    return 0;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);

    let nan = 0.0 / 0.0;
    let inf = 1.0 / 0.0;

    borrow mut io as &!i in {
        // Arithmetic, reported through `truncate`: this fixture predates
        // `std.fmt` and stays integral on purpose, so that what it checks
        // is the arithmetic rather than the printer.
        label(i, "sum", truncate((1.5 + 2.0) * 10.0));
        // `float_of` spells the widening; there is no implicit one.
        label(i, "half", truncate(float_of(1) / 2.0 * 10.0));
        // A negative literal is one literal, so this is -2.7 and not a
        // negation applied to 2.7 -- which matters for `-0.0`.
        label(i, "neg", truncate(-2.7 * 10.0));
        // 2, not 3: `truncate` goes toward zero and does not round,
        // which is the whole reason the builtin is named after its
        // rounding mode rather than after its result type (§4).
        label(i, "exponent", truncate(2.5e-3 * 1.0e3));

        // §5: NaN is not equal to itself, so `is_nan` exists.
        label(i, "nan-is-nan", flag(is_nan(nan)));
        label(i, "nan-equals-itself", flag(nan == nan));
        // §2: division by zero is infinity here, where the integer one
        // traps. Two divisions, two answers, both defined.
        label(i, "inf-beats-everything", flag(inf > 1.0e308));

        // §4: toward zero, so -2.7 truncates to -2 and not to -3.
        label(i, "toward-zero", truncate(-2.7));

        // §4.1: `bits_of` converts nothing. 1.0's sign, exponent and
        // mantissa laid end to end are this integer, and reading them
        // is what lets `std.fmt` print a float without the compiler's
        // help (`float-printing.md` §2).
        label(i, "bits-of-one", bits_of(1.0));
        // And the sign bit survives, which is why `-0.0` prints with a
        // sign rather than becoming zero on the way out. `§5` says
        // `-0.0 == 0.0` is true, so the bits are the only place the
        // difference is visible at all.
        label(i, "sign-of-minus-zero", flag(bits_of(-0.0) < 0));
        label(i, "minus-zero-equals-zero", flag(-0.0 == 0.0));
    }

    release(io);
    return 0;
}
