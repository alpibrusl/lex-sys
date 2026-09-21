//~ STDIN hello
//~ STDIN world
//~ STDOUT hello
//~ STDOUT world
//~ STDOUT 12 bytes

// `docs/standard-input.md` §3, and the first fixture with a `//~ STDIN`.
//
// `echo` reads to the end of input and writes what it read. Two things
// worth reading in its signature:
//
//   * the row is `[io_read, io_write]`, which says what the function does
//     in both directions. A caller learns from the type that this
//     consumes the program's input -- which is the point of the two
//     labels being two labels (§2.1).
//
//   * `-1` ends it. A byte is 0..255, so the sentinel cannot be one, and
//     the loop is the same shape C's has been since 1978. §3.1 says why
//     an enum would be better and why it is not that yet.
//
// Twelve bytes: "hello\nworld\n". The newlines are bytes like any other
// and the count says so.

fn echo[&i](io: &!i Io) -> [io_read, io_write] int {
    var read = 0;
    var c = getchar(io);
    while c >= 0 {
        putchar(io, c);
        read = read + 1;
        c = getchar(io);
    }
    return read;
}

fn print_nat[&i](io: &!i Io, n: int) -> [io_write] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, 48 + n % 10);
}

fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io_write] int {
    var n = 0;
    while n < len(s) {
        putchar(io, int_of(s[n]));
        n = n + 1;
    }
    return len(s);
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);

    var read = 0;
    borrow mut io as &!i in {
        read = echo(i);
        print_nat(i, read);
        write_all(i, " bytes");
        putchar(i, 10);
    }
    release(io);
    return read - 12;
}
