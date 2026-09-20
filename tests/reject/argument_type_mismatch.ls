//~ ERROR expected `int`, found `bool`

// The capability is fine; the character is not. An argument is checked
// against the parameter it is passed to, and `putchar`'s second one is the
// byte to write.

fn shout[&i](io: &!i Io) -> [io] int {
    putchar(io, true);
    return 0;
}

fn main(world: World) -> [] int {
    let Split { io, ffi } = split(world);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    release(io);
    return 0;
}
