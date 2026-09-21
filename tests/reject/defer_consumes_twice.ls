//~ ERROR has already been consumed

// `docs/defer.md` §3: the expansion is **real**, so a value the block
// also consumes by hand is consumed twice and refused by the ordinary
// rule rather than by anything `defer` brought with it.
//
// That is the whole design: `defer` is expanded during lowering into
// the statement it stands for, so the linear checker replays exactly
// the events it would have replayed for the hand-written version. A
// `defer` the checker knew about would be a second set of linearity
// rules to keep in agreement with the first.
//
// The diagnostic points at the `defer`, which is right: it is the one
// that runs second.

res struct File {
    fd: int,
}

fn open(n: int) -> [] File {
    return File { fd: n };
}

fn close(f: File) -> [] int {
    let File { fd } = f;
    return fd;
}

fn twice() -> [] int {
    let f = open(7);
    defer close(f);
    return close(f);
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
