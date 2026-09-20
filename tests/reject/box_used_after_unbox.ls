//~ ERROR consumed

// §3.1 from the other direction: `unbox` consumes the box, so there is no
// use-after-free to have. The same rule that refuses a second `release` of
// a capability refuses a second `unbox` of a box, and it is the same rule
// that makes double-free unexpressible.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap } = split(world);
    release(ffi);
    release(fs);
    release(io);

    var status = 0;
    borrow mut heap as &!h in {
        let b = box(h, 41);
        let first = unbox(h, b);
        // The allocation is gone and so is the box. Asking again is not a
        // runtime hazard here; it is a program that does not compile.
        let second = unbox(h, b);
        status = first + second;
    }
    release(heap);
    return status;
}
