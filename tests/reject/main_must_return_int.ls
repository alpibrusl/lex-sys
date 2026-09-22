//~ ERROR `main` returns `int`, the process exit status
//~ RULE program-shape

// The shape of `main` is a contract with the C runtime rather than a matter
// of taste: it takes the `World` and gives back the exit status.

fn main(world: World) -> [] bool {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // This program touches no files, so that authority ends here.
    release(heap);
    release(fs);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    release(io);
    return true;
}
