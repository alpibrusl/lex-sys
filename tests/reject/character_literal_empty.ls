//~ ERROR a character literal holds exactly one character
//~ RULE literal-form

// `docs/character-literals.md` §3: a character literal holds exactly one
// character, so there is no empty one. C spells this mistake the same way
// and rejects it too; what differs is that here the span is on the
// literal rather than on the statement around it.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    let nothing = '';
    return 0;
}
