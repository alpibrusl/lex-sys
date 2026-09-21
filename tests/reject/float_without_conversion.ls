// `docs/floating-point.md` §4: no implicit conversion, in either
// direction.
//
// The rule `strings.md` §2 already applies to `byte`: the conversion is
// where the decision lives, so it is written where a reader can see it.
// `float_of(n)` is what this line wants.
//~ ERROR expected `float`, found `int`

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    let mixed = 1.5 + 2;
    return truncate(mixed);
}
