//~ ERROR is still live

// §3: `unbox_slice` is the only consumer a boxed slice has, which is what
// keeps `heap.md` §3.1 true of the second shape as well.
//
// A boxed slice is `res`, so §4's exactly-once rule applies to it
// unchanged: one that is never ended is a compile error at the line that
// forgot it. The general heap still cannot leak.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(io);

    borrow mut heap as &!h in {
        // Four bytes, allocated and then forgotten.
        let buffer = box_slice(h, 4, byte_of(0));
    }
    release(heap);
    return 0;
}
