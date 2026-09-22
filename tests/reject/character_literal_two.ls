//~ ERROR a character literal holds exactly one character
//~ RULE literal-form

// `docs/character-literals.md` §4: C's multi-character literal is
// implementation-defined, which is the category this language exists to
// leave. A longer run of text is a string, and the error says so.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    let two = 'ab';
    return 0;
}
