//~ ERROR so it cannot be discarded
//~ RULE linear-value-unconsumed

// `docs/defer.md` §2: `defer E;` discards `E`'s value, so it obeys the
// rule an expression statement obeys — and discarding a `res` is a leak
// whether or not a `defer` is what discarded it.
//
// Worth a fixture because `defer` is the one place in the language
// where a statement is written in one block and runs at the exits of
// that block, and "which rules still apply over there" is exactly what
// a sugar feature gets wrong.

res struct File {
    fd: int,
}

fn open(n: int) -> [] File {
    return File { fd: n };
}

fn leak() -> [] int {
    defer open(1);
    return 0;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);
    release(io);
    return 0;
}
