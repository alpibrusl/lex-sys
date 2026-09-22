// `docs/character-literals.md`: the third spelling of an integer. `'a'` is
// 97 and nothing past the parser knows which one was written (§2), so the
// checks below are arithmetic and comparison on ordinary `int`s.
//
// The refusals are reject fixtures rather than lines here: an empty
// literal, two characters, a non-ASCII one and an unknown escape are all
// refused before the program exists.
//~ STDOUT a 97
//~ STDOUT zero 48
//~ STDOUT digit 55
//~ STDOUT newline 10
//~ STDOUT tab 9
//~ STDOUT return 13
//~ STDOUT nul 0
//~ STDOUT backslash 92
//~ STDOUT quote 39
//~ STDOUT double 34
//~ STDOUT space 32
//~ STDOUT tilde 126
//~ STDOUT same 1
//~ STDOUT byte 1
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
        label(i, "a", 'a');
        label(i, "zero", '0');

        // §2: the single most common site in the corpus, now legible --
        // the digit printer's `48 + n % 10`.
        label(i, "digit", '0' + 7);

        // The six escapes, which are `strings.md` §4's with `\'` where
        // `\"` is (§3).
        label(i, "newline", '\n');
        label(i, "tab", '\t');
        label(i, "return", '\r');
        label(i, "nul", '\0');
        label(i, "backslash", '\\');
        label(i, "quote", '\'');

        // Each literal escapes its own delimiter and not the other's, so
        // a double quote stands for itself here and needs no escape.
        label(i, "double", '"');

        label(i, "space", ' ');
        label(i, "tilde", '~');

        // §2 again, as a claim the program checks: the two spellings are
        // one value.
        var same = 0;
        if 'a' == 97 {
            same = 1;
        }
        label(i, "same", same);

        // §2.1: the `byte` sites keep their conversion and gain a legible
        // argument. This is what `text[i] == byte_of(10)` becomes.
        var byte_side = 0;
        if "a\n"[1] == byte_of('\n') {
            byte_side = 1;
        }
        label(i, "byte", byte_side);
    }

    release(io);
    return 0;
}
