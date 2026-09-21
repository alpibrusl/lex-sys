//~ ERROR takes 1 argument

// `docs/standard-input.md` §2: reading the console is authority, and
// authority is a parameter.
//
// There is no ambient stdin here any more than there is an ambient
// `Io::global()`. A function that reads what the user types has to hold
// the capability that says so, which means it has to say so in its
// parameter list, which means its callers know.

fn peek() -> [io_read] int {
    return getchar();
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);
    release(io);
    return peek();
}
