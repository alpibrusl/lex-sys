//~ ERROR `io` is still live here

// §8.3: authority is a resource, and a resource is destroyed exactly once.
// A program that forgets to release one does not compile -- by §4's rule,
// with nothing added for capabilities.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap } = split(world);
    // This program touches no files, so that authority ends here.
    release(heap);
    release(fs);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    return 0;
}
