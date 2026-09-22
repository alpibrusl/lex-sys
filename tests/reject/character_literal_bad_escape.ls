//~ ERROR is not an escape
//~ RULE unknown-escape

// `docs/character-literals.md` §3: the escape set is `strings.md` §4's
// six with `\\'` where `\\"` is. Anything else is a typo, and a typo is
// refused where it is written rather than passed through as itself --
// the same rule, and the same `unknown-escape` tag, that a string gets.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    let broken = '\q';
    return 0;
}
