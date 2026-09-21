// `base64` -- a port, and the first program here that already existed.
//
// The behaviour is GNU coreutils 9.4's, because that is what it was
// checked against: `base64` encodes standard input and wraps at 76
// columns, `base64 -d` decodes and ignores newlines. The conformance
// suite pipes real input through both this and `/usr/bin/base64` and
// compares the bytes, so "the same" is not a claim made here.
//
// Why this program. `docs/reach.md` §6 and `docs/overflow-cost.md` §4
// both wanted the same thing for different reasons -- a program with its
// own opinions, so the effect rows could be read *in anger* rather than
// off code written to make a point. `docs/porting.md` is what that found.
//
// What it needed that did not exist: the bit operators
// (`docs/bitwise.md`), which is the whole of RFC 4648 §4 in four lines,
// and hexadecimal literals to write a mask with. Nothing else.
//
// It **streams**. Three bytes in, four characters out, and no buffer of
// the input anywhere -- which is what the C does, and is also the only
// way it could work here, since an arena is one 64 KiB chunk
// (`docs/heap.md`) and standard input is not.

import std.io;

// ---------------------------------------------------------------------
// The alphabet
// ---------------------------------------------------------------------

// RFC 4648 §4. Written as a literal rather than computed, exactly as the
// C does, because the table *is* the specification.
fn alphabet() -> [] &static [byte] {
    return "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
}

// The inverse, as a 256-entry table — **built by the loop, at compile
// time** (`docs/compile-time-data.md`).
//
// This used to be the loop itself, run once per decoded character, and
// the comment here used to explain why: 64 entries of a 256-entry table
// is a lot of source for something a loop settles. Both halves of that
// are still true, and now neither costs anything, because the loop runs
// during compilation and the table is in the binary.
//
// It was worth 5.7× on 5.4 MB (§1 of that document), which makes it the
// largest single change to this program since it was ported.
static decode_table: [int] {
    let table = alloc_slice[static](256, 0 - 1);
    let alpha = alphabet();
    var i = 0;
    while i < len(alpha) {
        table[int_of(alpha[i])] = i;
        i = i + 1;
    }
    return table;
}

// Returns -1 for anything that is not in the alphabet, which is how the
// decoder tells padding and whitespace from a character it must refuse.
//
// Still pure, and `lex-sys authority` still says so: reading a `static`
// needs no parameter, which is the difference between this and passing
// the table down from `main` (§1.1).
fn value_of(c: int) -> [] int {
    if c < 0 || c >= len(decode_table) {
        return 0 - 1;
    }
    return decode_table[c];
}

// ---------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------

// One output character into the line buffer, wrapping at 76 columns the
// way coreutils does.
//
// This was a `putchar` straight to the console until `docs/bulk-io.md`,
// which measured a libc call per byte at 12.8× a bulk write. Filling a
// buffer and writing the run at once is what `write_bytes` is *for* —
// and §5 of that document is the note that a buffered writer is a
// library-shaped problem now rather than a language-shaped one.
//
// `at` and `column` both go in and come back out because there is
// nowhere else to keep them: this is a program, not an object, and
// threading two integers is what that costs.
fn emit[&o](out: &!o [byte], at: int, column: int, c: int) -> [] (int, int) {
    out[at] = byte_of(c);
    if column + 1 == 76 {
        out[at + 1] = byte_of(10);
        return (at + 2, 0);
    }
    return (at + 1, column + 1);
}

// Three bytes to four characters. `held` is 0, 1 or 2 bytes short of a
// group, and `bits` holds what has been read so far, most significant
// first -- which is RFC 4648 §4's own description, and is why the shifts
// read the way they do.
fn encode[&i](io: &!i Io) -> [io_read, io_write] int {
    let table = alphabet();
    var bits = 0;
    var held = 0;
    var column = 0;

    region o {
        // One buffer, flushed when a group might not fit. 4096 is a page
        // and the tail leaves room for a group plus its newline.
        let out = alloc_slice[o](4096, byte_of(0));
        var at = 0;

        var c = getchar(io);
        while c >= 0 {
            bits = (bits << 8) | c;
            held = held + 1;
            if held == 3 {
                let a = emit(out, at, column, int_of(table[(bits >> 18) & 0x3f]));
                let b = emit(out, a.0, a.1, int_of(table[(bits >> 12) & 0x3f]));
                let d = emit(out, b.0, b.1, int_of(table[(bits >> 6) & 0x3f]));
                let e = emit(out, d.0, d.1, int_of(table[bits & 0x3f]));
                at = e.0;
                column = e.1;
                if at > 4080 {
                    io.write_all(io, out[0..at]);
                    at = 0;
                }
                bits = 0;
                held = 0;
            }
            c = getchar(io);
        }

        // The tail. One held byte becomes two characters and two `=`; two
        // held bytes become three characters and one `=`. The shifts are
        // the same ones above with the missing bytes read as zero, which
        // is what the padding is *for*.
        if held == 1 {
            bits = bits << 16;
            let a = emit(out, at, column, int_of(table[(bits >> 18) & 0x3f]));
            let b = emit(out, a.0, a.1, int_of(table[(bits >> 12) & 0x3f]));
            let d = emit(out, b.0, b.1, 61);
            let e = emit(out, d.0, d.1, 61);
            at = e.0;
            column = e.1;
        }
        if held == 2 {
            bits = bits << 8;
            let a = emit(out, at, column, int_of(table[(bits >> 18) & 0x3f]));
            let b = emit(out, a.0, a.1, int_of(table[(bits >> 12) & 0x3f]));
            let d = emit(out, b.0, b.1, int_of(table[(bits >> 6) & 0x3f]));
            let e = emit(out, d.0, d.1, 61);
            at = e.0;
            column = e.1;
        }

        // A final newline unless the last line already ended with one,
        // which is what `column == 0` means here.
        if column != 0 {
            out[at] = byte_of(10);
            at = at + 1;
        }
        if at > 0 {
            io.write_all(io, out[0..at]);
        }
    }
    return 0;
}

fn decode[&i](io: &!i Io) -> [io_read, io_write] int {
    var bits = 0;
    var held = 0;
    var padded = false;

    var c = getchar(io);
    while c >= 0 {
        if c == 61 {
            padded = true;
        } else {
            if c != 10 && c != 13 {
                // A character after the padding is malformed even if it is
                // in the alphabet: `=` means the stream ended.
                if padded {
                    return 1;
                }
                let v = value_of(c);
                if v < 0 {
                    return 1;
                }
                bits = (bits << 6) | v;
                held = held + 1;
                if held == 4 {
                    putchar(io, (bits >> 16) & 0xff);
                    putchar(io, (bits >> 8) & 0xff);
                    putchar(io, bits & 0xff);
                    bits = 0;
                    held = 0;
                }
            }
        }
        c = getchar(io);
    }

    // Two leftover characters carry one byte, three carry two. One
    // leftover carries nothing and cannot happen in well-formed input.
    if held == 1 {
        return 1;
    }
    if held == 2 {
        putchar(io, (bits >> 4) & 0xff);
    }
    if held == 3 {
        putchar(io, (bits >> 10) & 0xff);
        putchar(io, (bits >> 2) & 0xff);
    }
    return 0;
}

// ---------------------------------------------------------------------

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // A codec, so it reads no files, allocates nothing and calls no C.
    // Three capabilities destroyed on three lines, and the row on every
    // function below is `[io_read, io_write]` and could not be anything
    // else.
    release(fs);
    release(heap);
    release(ffi);

    var decoding = false;
    borrow args as &g in {
        var n = 1;
        while n < arg_count(g) {
            let flag = arg(g, n);
            if len(flag) == 2 && int_of(flag[0]) == 45 && int_of(flag[1]) == 100 {
                decoding = true;
            }
            n = n + 1;
        }
    }
    release(args);

    var status = 0;
    borrow mut io as &!i in {
        if decoding {
            status = decode(i);
        } else {
            status = encode(i);
        }
    }
    release(io);
    return status;
}
