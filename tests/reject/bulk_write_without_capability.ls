//~ ERROR `shout` performs `io_write`

// `docs/bulk-io.md` §3.2: the bulk write is a second primitive behind the
// **same** capability, so it is authorised exactly as `putchar` is and
// declared exactly as `putchar` would be.
//
// The whole argument of that document is that a faster program must not
// be a more powerful one. This fixture is the other half of that claim:
// it must not be a *less* accountable one either. A function that writes
// a whole slice owes `io_write` in its row for the same reason a function
// that writes one byte does.

fn shout[&i](io: &!i Io) -> [] int {
    return write_bytes(io, "no row says so");
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    borrow mut io as &!i in {
        shout(i);
    }
    release(io);
    return 0;
}
