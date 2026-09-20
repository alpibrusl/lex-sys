//~ ERROR shared slice

// The same promise §5 makes about a shared reference's referent, one level
// out: a `&r [T]` says its elements will not change while it is lent, and a
// write through it would break that for every holder at once.
//
// `alloc_slice` hands back `&!a [T]`, which may be written through. Passing
// that where a shared slice is expected is the one coercion §6 added -- and
// it goes one way, which is what this fixture is about.

fn clobber[&r](xs: &r [int]) -> [] int {
    xs[0] = 99;
    return 0;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs } = split(world);
    // This program touches no files, so that authority ends here.
    release(fs);
    release(ffi);
    release(io);
    return 0;
}
