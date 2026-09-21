// `docs/compile-time-data.md` §3 — refuse, don't downgrade.
//
// A `static` has no runtime fallback: nothing runs at program start and
// `alloc_slice[static]` has no runtime meaning, so an evaluation that
// cannot finish is a refusal rather than a quiet retreat to building the
// table at startup. Here the index is past the end, which would trap.
//~ ERROR cannot be evaluated
//~ ERROR it traps

static bad: [int] {
    let table = alloc_slice[static](4, 0);
    table[4] = 1;
    return table;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    return len(bad);
}
