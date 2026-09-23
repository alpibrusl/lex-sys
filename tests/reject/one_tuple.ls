//~ ERROR a tuple has two components or more
//~ RULE pattern-shape

// `docs/tuples.md` §2.1.
//
// `(e)` is grouping, so the only spelling a one-tuple could have is a
// trailing comma. It parses -- the comma is what makes a tuple -- and is
// refused here for having one component, the same rule `()` meets in
// `empty_tuple_type.ls`. This fixture exists because the fuzzer
// (`docs/fuzzing.md`) wrote the program nobody had: an earlier version of
// this suite said a one-tuple could not be written at all.

fn one() -> [] int {
    let t = (1,);
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
