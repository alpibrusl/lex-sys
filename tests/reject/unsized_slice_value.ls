//~ ERROR has no size of its own

// `[T]` is a *referent*, not a value. Its length is a runtime value rather
// than part of its type, so there is nothing to put on a stack, in a struct
// field or in a register -- which is exactly why a slice is a reference and
// carries the length beside the pointer.
//
// Refused at the type rather than at the backend: a parameter the compiler
// cannot lay out is a mistake the author should hear about where they wrote
// it, not a crash in code generation.

fn total(xs: [int]) -> [] int {
    return 0;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs } = split(world);
    // This program touches no files, so that authority ends here.
    release(fs);
    release(ffi);
    release(io);
    return 0;
}
