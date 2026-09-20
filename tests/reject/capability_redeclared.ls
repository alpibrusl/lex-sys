//~ ERROR is a built-in type and cannot be redeclared

// A program that could declare its own `Io` could hand itself one. The
// prelude's capability types are as reserved as `int` is, and for a sharper
// reason.

res struct Io {
    fd: int,
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // This program touches no files, so that authority ends here.
    release(heap);
    release(fs);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    release(io);
    return 0;
}
