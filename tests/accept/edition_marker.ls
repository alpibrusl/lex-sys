// `docs/editions.md` §6.1: `edition 1;` names the language as it is
// today, so writing it changes nothing. A file that omits it is edition
// 1 anyway -- this fixture pins that the marker itself is a no-op, not
// that a program needs one.
//~ STDOUT one
//~ EXIT 0

edition 1;

import std.io;

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);

    borrow mut io as &!i in {
        io.write_all(i, "one");
        io.newline(i);
    }

    release(io);
    return 0;
}
