//~ ERROR ended by `unbox`

// §3 and §4.1: a `Box` has no fields on purpose.
//
// What it owns is an allocation, and a pattern that could name the pointer
// would be a way to end one without freeing it -- the same silent drop
// `capability_destructured.ls` refuses for authority. The function that
// ends a box is named, and it is `unbox`.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap } = split(world);
    release(ffi);
    release(fs);
    release(io);

    var status = 0;
    borrow mut heap as &!h in {
        let b = box(h, 41);
        let Box { } = b;
        status = 0;
    }
    release(heap);
    return status;
}
