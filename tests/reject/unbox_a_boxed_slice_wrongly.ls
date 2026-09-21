//~ ERROR is unsized, so `unbox` has nothing to hand back

// §3: `unbox` and `unbox_slice` are different operations, and they have
// to be.
//
// `unbox` hands back what the box held. `[byte]` is unsized -- that is
// what the length in a slice is for -- so there is nothing for it to hand
// back, and no type it could return. `unbox_slice` frees the allocation
// and answers how many elements it freed instead.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(io);

    var n = 0;
    borrow mut heap as &!h in {
        let buffer = box_slice(h, 4, byte_of(0));
        let back = unbox(h, buffer);
        n = 0;
    }
    release(heap);
    return n;
}
