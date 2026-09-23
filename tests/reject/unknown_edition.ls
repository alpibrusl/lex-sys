//~ ERROR unknown edition 2; the only edition today is 1
//~ RULE unknown-edition

// `docs/editions.md` §6.1.
//
// A file's `edition N;` marker names one of the editions this compiler
// knows. Edition 1 is the language as it is today and needs no marker
// at all; there is nothing later than it yet to opt into.

edition 2;

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);
    release(io);
    return 0;
}
