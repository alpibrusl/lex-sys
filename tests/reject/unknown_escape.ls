//~ ERROR is not an escape

// `docs/strings.md` §4: five escapes and no more.
//
// `\u` would be an encoding claim, and §1 declines to make one -- a string
// is bytes. `\x` is the bitwise escape hatch §2 is deferring along with the
// operators to write masks with. Anything else is a typo, and a typo is
// refused where it is written rather than passed through as itself, which
// is what C does and what makes `"\q"` mean `q` there.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // This program touches no files, so that authority ends here.
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    let path = "C:\\Users\\quinn";
    let broken = "\q";
    return 0;
}
