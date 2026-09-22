//~ ERROR is not a reference, so there is nothing for `*` to follow
//~ RULE not-a-reference

// §3: `*` follows a reference, and there has to be one to follow.
//
// Worth a fixture because `*` is also multiplication, and a language that
// quietly accepted `*x` on a non-reference would be one where a typo in a
// product reads as something else entirely.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    let n = 41;
    return *n;
}
