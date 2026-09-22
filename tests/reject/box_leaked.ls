//~ ERROR is still live
//~ RULE linear-value-unconsumed

// `docs/heap.md` §3.1, and the property the whole section is for.
//
// A `Box` is a linear resource, so §4's rule applies to it unchanged: it
// must be consumed exactly once on every path. A box that is never unboxed
// is a leak, and a leak is a *compile error* here rather than something a
// profiler finds later.
//
// This is what lets the document claim the general heap cannot leak. Not
// "should not" -- cannot, checked, before the program runs.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    release(ffi);
    release(fs);
    release(io);

    var status = 0;
    borrow mut heap as &!h in {
        // Allocated and then forgotten. There is no `free` to forget,
        // because the box itself is the obligation.
        let b = box(h, 41);
        status = 0;
    }
    release(heap);
    return status;
}
