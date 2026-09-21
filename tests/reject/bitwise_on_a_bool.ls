// `docs/bitwise.md` §1: `&` is not `&&`.
//
// Treating a `bool` as one bit would make the two differ only in whether
// they short-circuit, so a dropped ampersand would compile and mean
// almost the same thing -- which is the shape of bug that is found in
// production rather than in review.
//~ ERROR expected `int`, found `bool`

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    if true & false {
        return 1;
    }
    return 0;
}
