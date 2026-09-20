//~ ERROR is not a borrowed `Fs`

// `docs/filesystem.md` §1 and §2: the filesystem is reached through the
// capability that names a path, and through nothing else.
//
// This is the whole reason `fs_read` and `fs_write` are builtins rather
// than `extern fn` declarations. An `extern` would be gated by
// `Ffi("libc")` -- so a program holding the FFI capability could open any
// path it liked, and `Fs` would be decoration. The authority that guards
// the filesystem has to be the one that names the filesystem.
//
// Here the program has released its `Fs` and offers the console capability
// instead, which authorises writing to a terminal and nothing about disks.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs } = split(world);
    release(ffi);
    release(fs);

    var read = 0;
    region a {
        let buffer = alloc_slice[a](16, byte_of(0));
        borrow mut io as &!i in {
            read = fs_read(i, "/tmp/anything", buffer);
        }
    }
    release(io);
    return read;
}
