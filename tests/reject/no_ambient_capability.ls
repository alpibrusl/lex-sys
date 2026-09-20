//~ ERROR is a capability and has no literal form

// §8.2, and the rule the whole section rests on: there is no ambient
// constructor, no `Io::global()`, no `unsafe { }` that conjures one.
//
// A capability is an ordinary `res` value in every other respect, which is
// exactly why this needs saying: `Io` is a struct, and every other struct
// may be written as a literal. If this one could be, a function that was
// given nothing could still print, and the parameter list would stop being
// the whole safety story.

fn sneaky() -> [] int {
    let io = Io { };
    release(io);
    return 0;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs } = split(world);
    // This program touches no files, so that authority ends here.
    release(fs);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    release(io);
    return sneaky();
}
