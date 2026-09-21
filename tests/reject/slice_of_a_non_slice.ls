//~ ERROR is not a slice

// `docs/slicing.md` §1: `..` takes a range *of a run*, so there has to be
// a run. An `int` has no elements to take a range of, and the refusal
// says which type could not supply them rather than which operator was
// misused -- the type is the thing the programmer can fix.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);
    release(io);
    let n = 7;
    return len(n[0..2]);
}
