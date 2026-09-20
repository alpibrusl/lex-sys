// `docs/strings.md` §6: a `&r [byte]` crosses to C as a pointer *and* a
// separate length, because C has no notion of the pair.
//
// So `write(fd, ptr, len)` is expressible and `strlen(ptr)` is not: this
// design puts no NUL anywhere, and the functions that take an explicit
// length are the ones that cannot run off the end. That is the same
// argument the bounds check makes, applied at the boundary.
//
// The capability is still the only way in. `write` is reached through an
// `Ffi("libc")` narrowed from the root, and its row says so all the way up.
//~ STDOUT written straight to fd 1
//~ STDOUT and so was this
//~ EXIT 0

// libc's `ssize_t write(int fd, const void *buf, size_t n)`. The slice
// supplies the middle two arguments from its two leaves.
extern fn write[&f, &s](ffi: &f Ffi("libc"), fd: int, buf: &s [byte], n: int)
    -> [ffi("libc")] int;

fn say[&f, &s](libc: &f Ffi("libc"), line: &s [byte]) -> [ffi("libc")] int {
    return write(libc, 1, line, len(line));
}

fn main(world: World) -> [] int {
    let Split { io, ffi } = split(world);
    // This program never touches the console capability: it writes to the
    // file descriptor directly, through libc.
    release(io);
    let libc = narrow(ffi, "libc");

    var written = 0;
    borrow libc as &f in {
        written = say(f, "written straight to fd 1\n");
        // A buffer from an arena crosses exactly as a literal does: both
        // are `&r [byte]`, and C is handed a pointer and a length either way.
        region a {
            let source = "and so was this\n";
            // Exactly as long as what goes in it: a slice's length is the
            // length, and a buffer with a spare byte would print the spare.
            let line = alloc_slice[a](len(source), byte_of(0));
            var n = 0;
            while n < len(source) {
                line[n] = source[n];
                n = n + 1;
            }
            written = written + say(f, line);
        }
    }

    release(libc);
    return written - 41;
}
