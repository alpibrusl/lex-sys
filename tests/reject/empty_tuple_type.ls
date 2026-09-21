//~ ERROR a tuple has two components or more

// `docs/tuples.md` §2.1.
//
// `()` parses as a tuple and is refused for having no components. That is
// not a missing feature: whether a function may return *nothing* is a
// decision about the unit type, which the checker has internally and
// deliberately does not let source write. Answering it by accident, as a
// zero-component tuple, is how a language ends up with two ways to say
// nothing.
//
// There is no companion fixture for a one-tuple, because there is no way
// to write one: `(e)` is grouping and was grouping first.

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
