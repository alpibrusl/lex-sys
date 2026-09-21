// `docs/compile-time-data.md` §6: a `static` is for data.
//
// A scalar constant is already folded where it is written
// (`compile-time.md` §3), so allowing `static LIMIT: int` would be a
// second way to say one thing — and the second way would be the slower
// one, because it cannot participate in an expression.
//~ ERROR must be a slice

static limit: int {
    return 4096;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    return 0;
}
