// `docs/compile-time.md` §4. Every trap the language has, not just
// overflow: a zero divisor is as certain as an overflow when both
// operands are written down.
//~ ERROR this divides by zero

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    return 1 / 0;
}
