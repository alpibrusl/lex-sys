//~ ERROR performs `err_write`, which its row [io_write] does not declare

// `docs/standard-error.md` §3.1, and the fixture the separate label is
// for.
//
// The console is one capability with three labels now. If `[io_write]`
// covered a diagnostic, the two streams would be a spelling rather than
// a distinction -- and the distinction is the one a reader can act on,
// because `1>` and `2>` are two different redirections.
//
// §1.1 is what that buys, measured: a program that put its diagnostic on
// the output stream fed a line of prose to the `sort` downstream of it.
// A row that cannot tell the two apart cannot warn anyone about that.

fn complain[&i](io: &!i Io) -> [io_write] int {
    return write_err(io, "on the wrong stream\n");
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);

    borrow mut io as &!i in {
        complain(i);
    }
    release(io);
    return 0;
}
