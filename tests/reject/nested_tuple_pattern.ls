//~ ERROR a pattern does not nest

// `docs/tuples.md` §4. This language has no nested patterns anywhere --
// not in a struct pattern, not in a `match` arm -- and tuples are not the
// place to introduce them: a pattern language is its own design, and this
// slice is an aggregate.
//
// The answer is two statements, and the message says which two. That is
// worth more than "expected an identifier", which is what the parser said
// before it was taught to recognise this shape.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);
    release(io);

    let nested = ((1, 2), 3);
    let ((a, b), c) = nested;
    return a + b + c;
}
