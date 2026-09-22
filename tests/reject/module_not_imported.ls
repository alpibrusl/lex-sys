//~ ERROR is not an imported module here
//~ RULE module-not-imported

// `docs/modules.md` §4: a qualified name reaches another module only
// where this file has imported it. Reachability is per file and `pub`
// is not a substitute -- a name being public says it *may* be reached,
// not that this file has asked to reach it.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(ffi); release(fs); release(heap); release(args);
    borrow mut io as &!i in {
        io.write_all(i, "no import above\n");
    }
    release(io);
    return 0;
}
