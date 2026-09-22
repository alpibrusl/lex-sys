//~ ERROR this cannot be assigned to
//~ RULE not-a-place

// Assignment writes to a *place* -- a binding, or a field reached
// through a unique reference. An expression is a value, and there is
// nowhere to put one back.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(io); release(ffi); release(fs); release(heap); release(args);
    var n = 1;
    n + 1 = 2;
    return 0;
}
