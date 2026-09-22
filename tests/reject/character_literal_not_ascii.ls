//~ ERROR is not ASCII
//~ RULE literal-form

// `docs/character-literals.md` §3.1: `é` is U+00E9 and also the two
// bytes 0xC3 0xA9, and both are obvious readings of this literal.
// `strings.md` §1 declines to make an encoding claim about `[byte]`, so a
// literal that silently picked one would make it here instead. Refused,
// and the message names both ways out.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    let accented = 'é';
    return 0;
}
