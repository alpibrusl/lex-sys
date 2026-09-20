//~ ERROR shared reference

// §3: `*r = v` replaces the referent, so the reference has to be unique.
//
// A shared borrow is the promise that the value will not change while it
// is lent -- the same promise `write_through_shared_slice.ls` holds an
// indexing write to. Whether the write is spelled `s[i] =`, `r.field =` or
// `*r =`, the promise is the one being broken.

fn set[&r](target: &r int) -> [] int {
    *target = 99;
    return 0;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    var n = 1;
    var out = 0;
    borrow n as &r in {
        out = set(r);
    }
    return out;
}
