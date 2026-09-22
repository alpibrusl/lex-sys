// `docs/utf8.md` §1 said the rest of a string library is "code, not
// design". These are the four functions a program actually asked for:
// `examples/cut/` wanted `count_byte` and `field`, `examples/sort/`
// had `compare` inline, and `trim` came with the field parser.
//
// The empty-field cases are the ones worth pinning. `cut` meets them on
// every real CSV and they are where a hand-rolled splitter goes wrong.
//~ STDOUT count: 3 0 4
//~ STDOUT fields: [b] [] [d] [] []
//~ STDOUT edges: [] [a] [] [c]
//~ STDOUT trim: [x y] [] [] [a  b]
//~ STDOUT compare: -1 1 0 -1 1
//~ EXIT 0

import std.io;
import std.bytes;

fn show[&i, &s](io: &!i Io, s: &s [byte]) -> [io_write] int {
    io.write_all(io, "[");
    io.write_all(io, s);
    return io.write_all(io, "]");
}

fn sign(n: int) -> [] int {
    if n < 0 { return 0 - 1; }
    if n > 0 { return 1; }
    return 0;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(ffi);
    borrow mut io as &!i in {
        // count_byte
        io.write_all(i, "count:");
        io.space(i); io.print_int(i, bytes.count_byte("a,b,c,d", 44));
        io.space(i); io.print_int(i, bytes.count_byte("abcd", 44));
        io.space(i); io.print_int(i, bytes.count_byte(",,,,", 44));
        io.newline(i);

        // field: 1-based, empty where absent
        io.write_all(i, "fields:");
        io.space(i); show(i, bytes.field("a,b,c", 44, 2));
        io.space(i); show(i, bytes.field("a,b,c", 44, 9));
        io.space(i); show(i, bytes.field("a,b,c,d", 44, 4));
        io.space(i); show(i, bytes.field("a,b,c", 44, 0));
        io.space(i); show(i, bytes.field("", 44, 1));
        io.newline(i);

        // Empty fields at both ends and in the middle.
        io.write_all(i, "edges:");
        io.space(i); show(i, bytes.field(",a,,c", 44, 1));
        io.space(i); show(i, bytes.field(",a,,c", 44, 2));
        io.space(i); show(i, bytes.field(",a,,c", 44, 3));
        io.space(i); show(i, bytes.field(",a,,c", 44, 4));
        io.newline(i);

        // trim
        io.write_all(i, "trim:");
        io.space(i); show(i, bytes.trim("  x y \t\n"));
        io.space(i); show(i, bytes.trim("   "));
        io.space(i); show(i, bytes.trim(""));
        // Inner blanks survive; only the ends go.
        io.space(i); show(i, bytes.trim(" a  b "));
        io.newline(i);

        // compare: byte order, shorter first on a prefix
        io.write_all(i, "compare:");
        io.space(i); io.print_int(i, sign(bytes.compare("abc", "abd")));
        io.space(i); io.print_int(i, sign(bytes.compare("b", "a")));
        io.space(i); io.print_int(i, sign(bytes.compare("same", "same")));
        io.space(i); io.print_int(i, sign(bytes.compare("ab", "abc")));
        io.space(i); io.print_int(i, sign(bytes.compare("abc", "ab")));
        io.newline(i);
    }
    release(io);
    return 0;
}
