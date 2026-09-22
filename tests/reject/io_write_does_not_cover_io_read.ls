//~ ERROR performs `io_read`, which its row [io_write] does not declare
//~ RULE effect-not-declared

// `docs/standard-input.md` §2.1, and the fixture the whole rename is
// for.
//
// `Io` is one capability with two labels, the way `Fs` is one capability
// with `fs_read` and `fs_write`. If a row saying `[io_write]` let a
// function read, the two labels would be a spelling rather than a
// distinction, and a caller reading `[io_write]` would learn nothing
// about whether its input is being consumed.
//
// It does not. The refusal names the label that is missing, which is
// what makes the row worth reading.

fn sink[&i](io: &!i Io) -> [io_write] int {
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
        c = sink(i);
    }
    release(io);
    return 0;
}
