// `docs/aliasing.md` §3, route 2 again, and this is the version that
// scales: a `&!` reference is an ordinary `val`, so it can be the
// *element type* of a slice. One `alloc_slice` makes as many aliases of
// one object as the count asks for.
//
// A struct cannot hold one: a type declaration takes no region
// parameters, so `struct Pair[&a] { l: &!a [int] }` is refused outright,
// and a field can only name a region that needs no parameter -- which
// means `&static`, read-only data that aliases nothing an arena holds.
// That is the bound on how far a reference can travel. A slice is not
// bounded the same way, because `alloc_slice[r](n, fill)` takes its
// element type from `fill` rather than from a written annotation.
//
// Nothing in the corpus does this. It is here because the difference
// between "no program does it" and "no program can" is the whole
// question, and only a fixture keeps the answer honest.
//~ STDOUT 2 2 2 2
//~ EXIT 0

import std.io;

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

    region r {
        var s = alloc_slice[r](1, 0);
        // Four copies of one pointer.
        var many = alloc_slice[r](4, s);
        // Each write lands in the same element, so the last one wins...
        many[0][0] = 1;
        many[1][0] = 2;

        // ...and every alias reads it back.
        borrow mut io as &!i in {
            var n = 0;
            while n < 4 {
                if n > 0 {
                    io.write_all(i, " ");
                }
                let one = many[n];
                io.print_int(i, one[0]);
                n = n + 1;
            }
            io.newline(i);
        }
    }
    release(io);
    return 0;
}
