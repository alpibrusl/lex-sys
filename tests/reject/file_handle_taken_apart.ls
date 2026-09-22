//~ ERROR owns an open descriptor and is ended by `file_close`
//~ RULE linear-value-taken-apart

// `docs/file-handles.md` §2, and the same rule `release` and `unbox`
// already have: a value that owns something the language cannot see may
// not be taken apart, because a pattern naming the descriptor would end
// one without calling `close`.
//
// The leak a `Box` loses is the allocator's. This one is the kernel's,
// which keeps it for the life of the process and does not notice.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(heap);
    release(io);
    borrow fs as &c in {
        match open_read(c, "/tmp/lex-sys-apart.txt") {
            Opened::Ok(f) => {
                let File { } = f;
            }
            Opened::Failed(e) => {
            }
        }
    }
    release(fs);
    return 0;
}
