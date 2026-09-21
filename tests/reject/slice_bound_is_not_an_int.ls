//~ ERROR expected `int`, found `bool`

// `docs/slicing.md` §1: both bounds are `int`.
//
// Worth a fixture rather than being left to the type checker's general
// behaviour, because `s[a..b]` is the one place in the language where
// three expressions sit inside one pair of brackets, and "which of them
// is checked against what" is exactly the sort of thing a rewrite gets
// subtly wrong.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);
    release(io);
    let text = "hello";
    return len(text[0..true]);
}
