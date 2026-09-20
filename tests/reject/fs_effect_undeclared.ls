//~ ERROR does not declare

// §7.3 and `docs/filesystem.md` §1: the row is exact, and the label carries
// the prefix.
//
// `[fs_read]` is not a weaker version of `fs_read("/tmp/app")` -- it is a
// different label, and the point of putting the prefix in the effect is
// that a caller can read which part of the filesystem a function touches
// without opening its body. A row that dropped the prefix would take that
// away, so it is refused rather than accepted as an approximation.

fn read_config[&f, &b](fs: &f Fs("/tmp/app"), into: &!b [byte]) -> [] int {
    return fs_read(fs, "/tmp/app/config", into);
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs } = split(world);
    release(ffi);
    release(io);

    let app = narrow(fs, "/tmp/app");
    var read = 0;
    region a {
        let buffer = alloc_slice[a](16, byte_of(0));
        borrow app as &f in {
            read = read_config(f, buffer);
        }
    }
    release(app);
    return read;
}
