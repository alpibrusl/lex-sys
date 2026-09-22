// `docs/compile-time.md` §4 and `docs/bitwise.md` §3. The amount is
// outside `0..64` and it is written down, so the shift can only trap.
//
// The runtime half is a conformance test rather than a fixture, for the
// reason `slicing.md` §8 gives: a program that traps is one that
// compiled, and the reject harness runs `check`.
//~ ERROR this shifts by an amount outside `0..64`
//~ RULE constant-traps

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    return 1 << 64;
}
