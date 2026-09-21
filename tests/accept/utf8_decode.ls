// `docs/utf8.md` — decoding, and what an invalid sequence is.
//
// The malformed cases are the point. §2 found four implementations
// giving four different answers on exactly these bytes, so the counts
// below are §3.2's maximal-subpart rule rather than an accident of how
// the loop was written.
//
// The valid cases are written as **raw UTF-8 in this source file**.
// `strings.md` §8 declines `\u` escapes, and this is why that costs
// nothing: a string is bytes and the file is already UTF-8, so the
// bytes are simply there.
//~ STDOUT ascii: 1 valid
//~ STDOUT euro: 1 valid
//~ STDOUT emoji: 1 valid
//~ STDOUT combining: 2 valid
//~ STDOUT boundaries: 4 valid
//~ STDOUT overlong-nul: 2 invalid
//~ STDOUT surrogate: 3 invalid
//~ STDOUT above-max: 4 invalid
//~ STDOUT truncated: 1 invalid
//~ STDOUT lone-tail: 1 invalid
//~ EXIT 0

import std.io;
import std.utf8;

fn show[&i, &r, &t](io: &!i Io, label: &t [byte], text: &r [byte]) -> [io_write] int {
    io.write_all(io, label);
    io.write_all(io, ": ");
    io.print_int(io, utf8.count(text));
    if utf8.is_valid(text) { io.write_all(io, " valid"); }
    else { io.write_all(io, " invalid"); }
    return io.newline(io);
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(ffi);
    borrow mut io as &!i in {
        show(i, "ascii", "A");
        show(i, "euro", "€");
        show(i, "emoji", "😀");
        // `e` + combining acute: two code points, and one thing a
        // reader would call a character (§4).
        show(i, "combining", "é");
        // One code point at each width boundary: 1, 2, 2, 3 bytes.
        show(i, "boundaries", "߿ࠀ");

        region a {
            // `c0 80` — an overlong NUL. **Two** errors, not one: `c0`
            // can never begin a sequence, so neither byte is a maximal
            // subpart of anything longer.
            var bad = alloc_slice[a](2, byte_of(0));
            bad[0] = byte_of(0xc0); bad[1] = byte_of(0x80);
            show(i, "overlong-nul", bad);
        }
        region b {
            // `ed a0 80` — U+D800, a UTF-16 surrogate, which has no
            // UTF-8 encoding. Three errors: `ed` is a legal lead byte,
            // but `a0` is outside the range `ed` allows.
            var bad = alloc_slice[b](3, byte_of(0));
            bad[0] = byte_of(0xed); bad[1] = byte_of(0xa0); bad[2] = byte_of(0x80);
            show(i, "surrogate", bad);
        }
        region c {
            // `f5 80 80 80` — above U+10FFFF. GNU `wc -m` counts this
            // as one character, which is the row that disqualified it
            // as an oracle (§2).
            var bad = alloc_slice[c](4, byte_of(0));
            bad[0] = byte_of(0xf5); bad[1] = byte_of(0x80);
            bad[2] = byte_of(0x80); bad[3] = byte_of(0x80);
            show(i, "above-max", bad);
        }
        region d {
            // `e2 82` — the first two bytes of `€`. **One** error,
            // because the pair was a plausible beginning.
            var bad = alloc_slice[d](2, byte_of(0));
            bad[0] = byte_of(0xe2); bad[1] = byte_of(0x82);
            show(i, "truncated", bad);
        }
        region e {
            var bad = alloc_slice[e](1, byte_of(0));
            bad[0] = byte_of(0x80);
            show(i, "lone-tail", bad);
        }
    }
    release(io);
    return 0;
}
