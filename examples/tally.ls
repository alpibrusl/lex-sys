//~ STDIN the quick brown fox
//~ STDIN jumps over
//~ STDIN the lazy dog
//~ STDOUT       3      9     44

// `tally` — `wc` over standard input, and the program that could not be
// written before `docs/standard-input.md`.
//
// Everything else here reads a file it was told about, or a document
// compiled into it. This reads the one input every tool in a pipeline
// gets:
//
//     lex-sys run examples/tally.ls < some-file
//     cat some-file | ./tally
//
// Read `count`'s row. `[io_read, io_write]` says, in the signature, that
// this function consumes the program's input *and* writes to the
// console. A caller learns both without opening the body, which is what
// `arguments.md` §2 means by the whole capability story being about
// visibility rather than containment: one capability, `Io`, and two
// labels for the two directions, exactly as `Fs` has `fs_read` and
// `fs_write`.
//
// There is no `read_line` and no buffered read. `getchar` is the only
// primitive, and a word boundary is a policy -- where one ends, what
// counts as blank -- which does not belong in the compiler (§3.2).
//
// It used to live *here*, in a private `is_blank`, and the comment on
// it argued that a policy belongs in the program that holds it. That
// argument was against the **compiler**, and it was made before there
// was a library to be the third option. `std.bytes.is_blank` is that
// third option and it is the same six bytes.
//
// Keeping the copy was not free. `standard-library.md` §5.3 said this
// file and `wordcount.ls` "each had their own idea of a word boundary
// and they were not the same", and claimed "one definition, in one
// place" as the fix. `wordcount.ls` moved; this file did not, so there
// were still two definitions. They happened to agree, and nothing
// checked that they did.

// No tuples in the return here -- three counts and a `res`-free struct
// reads better than `(int, int, int)`, which is the honest use of
// `docs/tuples.md` §4: a tuple is for a pair that has no name, not for
// three things that do.
import std.bytes;

val struct Counts {
    lines: int,
    words: int,
    bytes: int,
}

fn count[&i](io: &!i Io) -> [io_read] Counts {
    var lines = 0;
    var words = 0;
    var bytes = 0;
    // Whether the previous byte was inside a word. A word is counted at
    // the moment it starts, which is the only way to count them in one
    // pass without looking ahead.
    var inside = false;

    var c = getchar(io);
    while c >= 0 {
        bytes = bytes + 1;
        if c == 10 {
            lines = lines + 1;
        }
        if bytes.is_blank(c) {
            inside = false;
        } else {
            if inside == false {
                words = words + 1;
            }
            inside = true;
        }
        c = getchar(io);
    }

    return Counts { lines: lines, words: words, bytes: bytes };
}

fn print_nat[&i](io: &!i Io, n: int) -> [io_write] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, 48 + n % 10);
}

// How many digits a count takes, so the columns line up. The column
// widths are this program's own: `wc` picks a common width from the
// largest count, which is a second pass this does not need.
//
// On the counts themselves, against GNU `wc` over this repository's
// README (758 lines, 42158 bytes): lines and bytes agree exactly, and
// words agree under a UTF-8 locale. Under the **C locale** `wc` reports
// 98 fewer, because it decodes each multi-byte sequence and skips the
// ones the locale calls invalid -- so ` -- ` between two spaces is not a
// word to it and is one to this.
//
// That difference is a position, not a bug. `docs/strings.md` §1: a
// string here is **bytes, not an encoding**. This program has no decoder
// and no locale, so a run of non-blank bytes is a word whatever those
// bytes mean. A `wc` that wanted the other answer would need to know
// what an encoding is, and nothing in this language does.
fn width(n: int) -> [] int {
    var digits = 1;
    var rest = n / 10;
    while rest > 0 {
        digits = digits + 1;
        rest = rest / 10;
    }
    return digits;
}

fn column[&i](io: &!i Io, n: int) -> [io_write] int {
    var pad = 7 - width(n);
    while pad > 0 {
        putchar(io, 32);
        pad = pad - 1;
    }
    return print_nat(io, n);
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);

    borrow mut io as &!i in {
        let counts = count(i);
        column(i, counts.lines);
        column(i, counts.words);
        column(i, counts.bytes);
        putchar(i, 10);
    }
    release(io);
    return 0;
}
