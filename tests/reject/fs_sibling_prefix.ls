//~ ERROR a path prefix extends at a `/`

// `docs/filesystem.md` §1: a path prefix is not a byte prefix.
//
// `/tmp` contains `/tmp/app`. It does not contain `/tmpevil`, which shares
// four bytes with it and is a different directory entirely. A refinement
// check that only compared bytes would hand this program the directory
// next door -- so the extension has to land on a separator, here and again
// at run time when a path is handed to an operation (§4).

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // This program allocates nothing on the heap, so that authority ends here.
    release(heap);
    release(ffi);
    release(io);

    let tmp = narrow(fs, "/tmp");
    let sideways = narrow(tmp, "/tmpevil");
    release(sideways);
    return 0;
}
