//~ ERROR performs `file_read`, which its row
//~ RULE effect-not-declared

// `docs/file-handles.md` §4.1: `read` performs a label, and a function
// that borrows a handle has to say so.
//
// The interesting half is what is *not* here. The row is `[file_read]`
// and never `[fs_read("/some/path")]`, because the path was spent at
// `open_read` -- so this function can be handed a handle by a caller
// that will not tell it where the file is, which is the whole reason a
// descriptor is worth having.

fn peek[&h, &b](h: &!h File, into: &!b [byte]) -> [] int {
    match file_read(h, into) {
        Read::Got(n) => { return n; }
        Read::End => { return 0; }
        Read::Failed(e) => { return e; }
    }
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(heap);
    release(io);
    release(fs);
    return 0;
}
