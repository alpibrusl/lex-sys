// `docs/reach.md` §3.1: a foreign *result* is a scalar, and that is the
// rule the whole reach argument turns on. C's `getenv` returns a `char *`
// -- a pointer with no length, no region and no provenance the checker
// can name -- so there is nothing here for it to come back as.
//
// This is also why an OpenSSL or a libpq client is out of reach today:
// both hand back an opaque handle, and a handle is a pointer.
//
// The message names `int` and `bool` and nothing else. It used to offer
// `()` as a third, which sent a reader to write `()` and be told there is
// no `()` -- `tuples.md` §4 keeps unit out of the grammar on purpose.
//~ ERROR a foreign result is `int` or `bool`

extern fn getenv[&f, &n](ffi: &f Ffi("libc"), name: &n [byte])
    -> [ffi("libc")] &n [byte];

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(io);
    release(ffi);
    return 0;
}
