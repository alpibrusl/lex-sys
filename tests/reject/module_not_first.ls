//~ ERROR a `module` declaration is the first item in its file
//~ RULE program-shape

// `docs/modules.md` §3.
//
// A file's module is a fact about the whole file, so it is written where
// it can be read first. Allowing it later would mean the same file had
// two answers to "what is in this namespace" depending on where you
// started reading.

fn early() -> [] int {
    return 1;
}

module std.late;

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);
    release(io);
    return 0;
}
