// `docs/bulk-io.md`: a whole slice in one call, behind the same
// capability one byte needed.
//
// The point is the *ordering* as much as the speed. `putchar` goes
// through stdio, so a bulk write had to use the same stream — POSIX
// `write` on descriptor 1 would have interleaved wrongly with it, and
// that is why the primitive is `fwrite` underneath (§3).
//~ STDOUT <hello, bulk>
//~ STDOUT one two three
//~ STDOUT empty:[] still here
//~ STDOUT sliced: ell
//~ EXIT 0

import std.io;

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);

    borrow mut io as &!i in {
        // Mixed with `putchar`, in order.
        putchar(i, 60);
        write_bytes(i, "hello, bulk");
        putchar(i, 62);
        putchar(i, 10);

        // Through `std.io`, which is one call now rather than a loop.
        io.write_all(i, "one ");
        io.write_all(i, "two ");
        io.write_all(i, "three");
        io.newline(i);

        // An empty slice writes nothing and is not an error: the length
        // is zero and `fwrite` is told so.
        io.write_all(i, "empty:[");
        region a {
            let nothing = alloc_slice[a](0, byte_of(0));
            write_bytes(i, nothing);
        }
        io.write_all(i, "] still here");
        io.newline(i);

        // A subslice, which is a pointer and a length like any other
        // (`slicing.md` §1) — so it crosses to libc without copying.
        io.write_all(i, "sliced: ");
        let word = "hello";
        write_bytes(i, word[1..4]);
        io.newline(i);
    }

    release(io);
    return 0;
}
