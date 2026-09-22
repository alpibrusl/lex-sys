//~ ERROR is a local binding, not a function
//~ RULE not-a-function

// `docs/reach.md` §3.3: there are no function values, so a name in call
// position is a function in this program or it is nothing. A binding
// that happens to hold an `int` is not a thing to call, and the
// refusal says which of the two it found.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(io); release(ffi); release(fs); release(heap); release(args);
    let value = 7;
    return value(1);
}
