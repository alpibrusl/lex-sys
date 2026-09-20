//~ ERROR is not a uniquely borrowed `Heap`

// `docs/heap.md` §2: allocating is reached through the capability that
// authorises it, and through nothing else.
//
// Same rule as the filesystem's, for the same reason (`filesystem.md` §2):
// `box` is a builtin rather than an `extern fn`, because an `extern` would
// be gated by `Ffi("libc")` and then the FFI capability would allocate,
// with `Heap` contributing nothing.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap } = split(world);
    release(ffi);
    release(fs);
    release(heap);

    var n = 0;
    borrow mut io as &!i in {
        // The console capability authorises writing to a terminal. It says
        // nothing about memory.
        let b = box(i, 41);
        n = unbox(i, b);
    }
    release(io);
    return n;
}
