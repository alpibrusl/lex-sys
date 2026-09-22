//~ ERROR performs `io_read`
//~ RULE effect-not-declared

// §7.2: an effect performed is an effect declared. The new label is not
// exempt.
//
// `quiet` holds an `Io` and reads from it, and its row says `[]`. That is
// a function claiming to be pure while consuming the program's input,
// which is exactly the claim `arguments.md` §2 says a row exists to stop
// anyone making.

fn quiet[&i](io: &!i Io) -> [] int {
    return getchar(io);
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);

    var c = 0;
    borrow mut io as &!i in {
        c = quiet(i);
    }
    release(io);
    return 0;
}
