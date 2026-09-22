// `docs/strings.md` §5: building a string is allocating a slice.
//
// Nothing here is string machinery. `alloc_slice`, `len`, `s[i]` and
// `s[i] = v` are what §6's arenas and M3's slices already did, and the only
// new thing is that the element type is `byte` -- which is packed one per
// byte (§3), so what comes out is a buffer C could read.
//~ STDOUT ABCDE
//~ STDOUT 5 bytes, 2 vowels
//~ EXIT 0

fn space[&i](io: &!i Io) -> [io_write] int {
    return putchar(io, 32);
}

fn print_nat[&i](io: &!i Io, n: int) -> [io_write] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, '0' + n % 10);
}

fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io_write] int {
    var n = 0;
    while n < len(s) {
        putchar(io, int_of(s[n]));
        n = n + 1;
    }
    return len(s);
}

// Comparing storage is not arithmetic, so `==` on bytes is allowed (§2).
fn is_vowel(b: byte) -> [] bool {
    return b == byte_of('A') || b == byte_of('E') || b == byte_of('I')
        || b == byte_of('O') || b == byte_of('U');
}

fn count_vowels[&r](s: &r [byte]) -> [] int {
    var found = 0;
    var n = 0;
    while n < len(s) {
        if is_vowel(s[n]) {
            found = found + 1;
        }
        n = n + 1;
    }
    return found;
}

fn run[&i](io: &!i Io) -> [io_write] int {
    var vowels = 0;
    var bytes = 0;
    region a {
        // Five bytes, all `A` to begin with.
        let buffer = alloc_slice[a](5, byte_of('A'));

        // Written through the unique slice the arena handed back. The
        // arithmetic happens in `int`, and `byte_of` is where the range is
        // checked -- visibly, rather than inside an operator.
        var n = 0;
        while n < len(buffer) {
            buffer[n] = byte_of('A' + n);
            n = n + 1;
        }

        write_all(io, buffer);
        putchar(io, 10);

        // Read through functions that only asked to borrow.
        bytes = len(buffer);
        vowels = count_vowels(buffer);
    }
    // The buffer is gone by here, with its arena, in one call.

    print_nat(io, bytes);
    space(io);
    write_all(io, "bytes,");
    space(io);
    print_nat(io, vowels);
    space(io);
    write_all(io, "vowels\n");
    return bytes * 10 + vowels;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // This program touches no files, so that authority ends here.
    release(heap);
    release(fs);
    release(ffi);
    var status = 0;
    borrow mut io as &!i in {
        status = run(i);
    }
    release(io);
    return status - 52;
}
