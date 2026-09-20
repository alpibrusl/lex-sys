//~ ERROR shared slice

// `docs/strings.md` §4: two occurrences of `"ok"` may be the same bytes, so
// a program that could write through one would be writing through both --
// and the data is in a read-only section, which would refuse the write
// anyway.
//
// So a literal is `&static [byte]`: shared, never unique. A buffer you may
// write to comes from `alloc_slice`, which hands back a unique slice.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap } = split(world);
    // This program touches no files, so that authority ends here.
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    let greeting = "hello";
    greeting[0] = byte_of(72);
    return 0;
}
