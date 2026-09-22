// `docs/strings.md` §2: a `byte` has no arithmetic, and a mask is
// arithmetic.
//
// The point is not that masking a byte is wrong -- it is what
// `bitwise.md` §1 says to do. It is that the conversion is written, so
// the range check has a place: `byte_of(int_of(b) & 15)`.
//~ ERROR expected `int`, found `byte`
//~ RULE type-mismatch

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    let b = byte_of(200);
    let low = b & byte_of(15);
    return int_of(low);
}
