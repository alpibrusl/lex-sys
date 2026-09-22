//~ ERROR is not a tuple
//~ RULE not-a-tuple

// `docs/tuples.md` §2: `let (a, b) = e` takes a tuple apart, and the
// shape is checked rather than assumed. An `int` has no components, so
// there is nothing for the two names to bind.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(io); release(ffi); release(fs); release(heap); release(args);
    let (a, b) = 7;
    return a + b;
}
