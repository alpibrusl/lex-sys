// `main`'s result is the process exit status, truncated to what the platform
// carries. Nothing is printed.
//~ EXIT 7

fn run() -> [] int {
    return 7;
}

fn main(world: World) -> [] int {
    // Nothing here prints, but the `World` still has to be accounted for:
    // authority is a resource whether or not it is used.
    let Split { io, ffi } = split(world);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    release(io);
    return run();
}
