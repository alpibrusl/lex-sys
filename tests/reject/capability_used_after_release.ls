//~ ERROR nothing left to borrow

// §8.3: a capability is a resource, so §4.1's use-after-move is what stops
// a released one being used again. No new rule, and no special case for
// authority -- that is the point of making a capability an ordinary value.

fn greet[&i](io: &!i Io) -> [io] int {
    return putchar(io, 65);
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // This program touches no files, so that authority ends here.
    release(heap);
    release(fs);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    release(io);
    borrow mut io as &!i in {
        greet(i);
    }
    return 0;
}
