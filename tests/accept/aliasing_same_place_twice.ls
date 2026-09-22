// `docs/aliasing.md` §3, route 1: two `&!` parameters, one object.
//
// This is the four-line falsification the README carries, kept as a
// fixture so that the day `&!` does start to mean unique, this file is a
// red build rather than a silent semantic change. If a rule ever refuses
// it, move it to `tests/reject/` on purpose.
//
// `&!` is a lock on the *binding* (`slicing.md` §4), not a no-aliasing
// invariant over references. `s` is already a `&!r [int]` -- `alloc_slice`
// hands one back -- and a reference is `val`, so passing it twice passes
// two copies of one pointer. The writes alias: `p[0] = 1` then `q[0] = 2`
// and the read sees 2, not 1.
//
// Across the 82-program corpus this is the only shape that could have
// aliased and does: 34 call sites pass two or more unique references and
// none of them passes the same place twice (`aliasing.md` §2).
//~ STDOUT 2
//~ EXIT 0

import std.io;

fn both[&a, &b](p: &!a [int], q: &!b [int]) -> [] int {
    p[0] = 1;
    q[0] = 2;
    return p[0];
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    // This program touches no files, so that authority ends here.
    release(fs);
    // Everything it allocates is in a region, so the heap ends here too.
    release(heap);

    var answer = 0;
    region r {
        var s = alloc_slice[r](4, 0);
        answer = both(s, s);
    }

    borrow mut io as &!i in {
        io.print_int(i, answer);
        io.newline(i);
    }
    release(io);
    return 0;
}
