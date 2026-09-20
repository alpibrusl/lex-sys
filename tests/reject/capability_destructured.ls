//~ ERROR destroyed by `release`, not by being taken apart

// `Io` carries no fields, so taking it apart would end a capability and
// produce nothing -- destroying authority without naming the function that
// knows how (§4.1). `release` is that function, and it is the only one.
//
// `Split` is the exception and is destructured on purpose: taking it apart
// is how its parts are reached.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap } = split(world);
    // This program touches no files, so that authority ends here.
    release(heap);
    release(fs);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    let Io { } = io;
    return 0;
}
