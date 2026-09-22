//~ ERROR is still live at the end of this block
//~ RULE linear-value-unconsumed

// `docs/file-handles.md` §2: the first of `filesystem.md` §3's three
// questions, and it needed no new machinery.
//
// §3 worried that a handle reaching the end of a block would leak the
// descriptor. It cannot: a `res` value that nothing consumes is already
// a compile error, and a descriptor is a `res` value. The worry was
// about *regions* freeing memory out from under one, and a region frees
// arena allocations -- a linear value has to be consumed by name
// wherever it lives.
//
// So this is the fixture for a rule the language already had, pointed at
// the newest thing that has it.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(heap);
    release(io);
    borrow fs as &c in {
        match open_read(c, "/tmp/lex-sys-unclosed.txt") {
            Opened::Ok(f) => {
                // `f` is an open descriptor and nothing ends it.
            }
            Opened::Failed(e) => {
            }
        }
    }
    release(fs);
    return 0;
}
