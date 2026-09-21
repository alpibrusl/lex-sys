//~ ERROR expected `int`, found `bool`

// The capability is fine; the character is not. An argument is checked
// against the parameter it is passed to, and `putchar`'s second one is the
// byte to write.

fn shout[&i](io: &!i Io) -> [io_write] int {
    putchar(io, true);
    return 0;
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
