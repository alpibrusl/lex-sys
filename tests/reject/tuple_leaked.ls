//~ ERROR is still live at the end of this block

// `docs/tuples.md` §2.3: a tuple is `res` if any component is, and every
// obligation that follows from `res` follows here.
//
// Nothing in the program says so. There is no `res` keyword on a tuple and
// nowhere to put one -- the mode is read off the components, because a
// tuple has no declaration site to write it at. This fixture is the proof
// that the computed mode is a real mode and not a notation: the value is
// refused for exactly what a `res struct` would be refused for.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(io);

    borrow mut heap as &!h in {
        let pair = (box(h, 41), 1);
    }
    release(heap);
    return 0;
}
