// `docs/aliasing.md` §3, route 2: one object, two bindings.
//
// The call-site rule that refuses `both(s, s)` -- "no two `&!` arguments
// may share a root place" -- costs nothing to enforce (no program in the
// corpus would be refused by it) and buys nothing, because these two
// lines route around it. `t` is a different binding holding the same
// pointer, so `both(s, t)` has two roots and one object.
//
// Closing this means a `&!` reference stops being `val`: copying one has
// to be a *move*, and every ordinary use has to become an implicit
// reborrow instead. The corpus reads a `&!` binding in 1,943 distinct
// places, so the reborrow is mandatory rather than an optimisation
// (`aliasing.md` §4.2). The copy itself is nearly free to give up: two
// places in 82 programs did it before this file joined them, and both
// are in `tests/accept/unique_borrow.ls`, whose comment is about
// exactly this.
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
        // One pointer, two names. Nothing about `t` says where it came
        // from -- that is the whole difficulty.
        let t = s;
        answer = both(s, t);
    }

    borrow mut io as &!i in {
        io.print_int(i, answer);
        io.newline(i);
    }
    release(io);
    return 0;
}
