//~ ERROR a tuple has two components or more
//~ RULE pattern-shape

// `docs/tuples.md` §2.1.
//
// `()` parses as a tuple and is refused for having no components. That is
// not a missing feature: whether a function may return *nothing* is a
// decision about the unit type, which the checker has internally and
// deliberately does not let source write. Answering it by accident, as a
// zero-component tuple, is how a language ends up with two ways to say
// nothing.
//
// The companion is `one_tuple.ls`: `(e)` is grouping and was grouping
// first, so a one-tuple could only be spelled `(e,)`, and that is refused
// the same way.

fn nothing() -> [] () {
    return 0;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);
    release(io);
    return 0;
}
