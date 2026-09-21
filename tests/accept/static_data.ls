// `docs/compile-time-data.md`: data computed during compilation and read
// as read-only bytes.
//
// Nothing here runs at program start. Every table below is in the
// binary's `.rodata` before `main` is entered, and the loops that built
// them ran in the compiler.
//~ STDOUT squares 0 1 4 9 16 25 36 49 64 81
//~ STDOUT shifted HELLO
//~ STDOUT doubled 0 2 8 18 32 50 72 98 128 162
//~ STDOUT pure-read 25
//~ EXIT 0

import std.io;

// A table built by a loop, which is the shape a decode table has.
static squares: [int] {
    let table = alloc_slice[static](10, 0);
    var i = 0;
    while i < 10 {
        table[i] = i * i;
        i = i + 1;
    }
    return table;
}

// A `[byte]` static is packed one byte per element, like every other
// byte slice — the backend uses the same `stride` it always did.
static shifted: [byte] {
    let out = alloc_slice[static](5, byte_of(0));
    let source = "hello";
    var i = 0;
    while i < len(source) {
        out[i] = byte_of(int_of(source[i]) - 32);
        i = i + 1;
    }
    return out;
}

// A `static` may read one declared **before** it, which is the cheapest
// rule with no cycles in it (§2). It may also call any pure function,
// through the same evaluator that folds `factorial(5)`.
fn twice(n: int) -> [] int {
    return n * 2;
}

static doubled: [int] {
    let table = alloc_slice[static](len(squares), 0);
    var i = 0;
    while i < len(table) {
        table[i] = twice(squares[i]);
        i = i + 1;
    }
    return table;
}

// Reading a `static` needs no parameter, so a function that reads one
// stays pure — which is the difference between this and threading the
// table down from `main` (§1.1). `lex-sys authority` lists this one.
fn square_of(n: int) -> [] int {
    if n < 0 || n >= len(squares) {
        return 0 - 1;
    }
    return squares[n];
}

fn row[&i](io: &!i Io, name: &static [byte], table: &static [int]) -> [io_write] int {
    io.write_all(io, name);
    var n = 0;
    while n < len(table) {
        io.space(io);
        io.print_int(io, table[n]);
        n = n + 1;
    }
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
        row(i, "squares", squares);
        io.write_all(i, "shifted ");
        io.write_all(i, shifted);
        io.newline(i);
        row(i, "doubled", doubled);
        io.write_all(i, "pure-read ");
        io.print_int(i, square_of(5));
        io.newline(i);
    }

    release(io);
    return 0;
}
