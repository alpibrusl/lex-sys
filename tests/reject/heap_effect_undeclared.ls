//~ ERROR does not declare

// §7.3 and `docs/heap.md` §2: the row is exact, and allocation is an
// effect like any other.
//
// This is the epic's first thesis sentence made literal -- "allocation is
// an effect, a heap value is a linear resource, `free` consumes it". A
// function that allocates says `heap` in its row, and a caller reads that
// without opening the body.

fn stash[&h](heap: &!h Heap, n: int) -> [] Box[int] {
    return box(heap, n);
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap } = split(world);
    release(ffi);
    release(fs);
    release(io);

    var status = 0;
    borrow mut heap as &!h in {
        let b = stash(h, 41);
        status = unbox(h, b);
    }
    release(heap);
    return status - 41;
}
