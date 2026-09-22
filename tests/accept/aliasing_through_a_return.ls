// `docs/aliasing.md` §3, route 3: a reference laundered through a call.
//
// `head` returns a slice of what it was given, so `head(s)` and `s` are
// the same object arriving at `both` by two different expressions. This
// is the route the other two cannot be fixed the same way as: the caller
// cannot see it, because nothing in `head`'s signature says the result
// borrows its argument.
//
// Regions do not say it either, and that is worth being exact about.
// `head[&a](s: &!a [int]) -> [] &!a [int]` says the result lives in the
// same *arena* as the argument -- and so does every other reference into
// that arena, including ones with nothing to do with `s`. Region identity
// is not provenance. Tracking provenance across a call boundary is a
// borrow checker, which `README.md`'s design commitments rule out by
// name (`aliasing.md` §5).
//
// `std.buffer.room` is this shape, and it is the only function in the
// corpus that is: refusing the shape outright would cost the standard
// library a function that `examples/sort/` needs (`aliasing.md` §4.3).
//~ STDOUT 2
//~ EXIT 0

import std.io;

fn head[&a](s: &!a [int]) -> [] &!a [int] {
    return s[0..1];
}

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
        answer = both(head(s), s);
    }

    borrow mut io as &!i in {
        io.print_int(i, answer);
        io.newline(i);
    }
    release(io);
    return 0;
}
