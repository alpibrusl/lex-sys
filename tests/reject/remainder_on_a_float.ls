// `docs/floating-point.md` §2: `+ - * /` are `int` or `float`; `%` is
// `int` only.
//
// There is no `frem` primitive worth the name -- C's `fmod` is a library
// call with its own rounding story and its own error cases -- so it
// belongs in `std.math` when floats get one, where it can say which
// rounding it does.
//~ ERROR expected `int`, found `float`
//~ RULE type-mismatch

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    let left = 5.5 % 2.0;
    return truncate(left);
}
