// `docs/floating-point.md` §1: a literal needs digits on both sides of
// the point, or an exponent.
//
// `1.` reads as a typo more often than as a number, so it is refused
// rather than accepted as `1.0`. What it lexes as is an integer followed
// by a dot, which is the start of a field access with nothing after it.
//~ ERROR expected an identifier
//~ RULE type-mismatch

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    let half = 1.;
    return truncate(half);
}
