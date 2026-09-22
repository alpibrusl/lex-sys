//~ ERROR `complain` performs `err_write`

// `docs/standard-error.md` §2: standard error is not a seventh
// capability, so it is authorised by the `Io` a program already holds --
// and *declared* exactly as every other use of that capability is.
//
// The mirror of `bulk_write_without_capability.ls`. §2.1 admits that a
// grant of `Io` is worth more after this change than before it, and the
// answer it gives is that the grant is the capability while the gate is
// the row. That answer is only worth anything if the row is compulsory,
// which is what this fixture holds to.

fn complain[&i](io: &!i Io) -> [] int {
    return write_err(io, "no row says so\n");
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    borrow mut io as &!i in {
        complain(i);
    }
    release(io);
    return 0;
}
