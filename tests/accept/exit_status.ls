// `main`'s result is the process exit status, truncated to what the platform
// carries. Nothing is printed.
//~ EXIT 7

fn run() -> [] int {
    return 7;
}

fn main(world: World) -> [] int {
    // Nothing here prints, but the `World` still has to be accounted for:
    // authority is a resource whether or not it is used.
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // This program touches no files, so that authority ends here.
    release(heap);
    release(fs);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    release(io);
    return run();
}
