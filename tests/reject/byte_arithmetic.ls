//~ ERROR expected `int`, found `byte`

// `docs/strings.md` §2: a `byte` is *storage*, not arithmetic.
//
// This is what keeps `defined-behaviour.md` §8's deferral of unsigned
// integers intact: a type you cannot add to never asks whether adding
// traps, wraps or widens. Convert, compute, convert back --
// `byte_of(int_of(b) + 1)` -- and the range check is visible where it
// happens instead of hidden in an operator.
//
// `==` is allowed, because comparing storage is not arithmetic and a parser
// that cannot say `b == byte_of(44)` is not worth having.

fn next(b: byte) -> [] byte {
    return b + byte_of(1);
}

fn main(world: World) -> [] int {
    let Split { io, ffi } = split(world);
    release(ffi);
    release(io);
    return 0;
}
