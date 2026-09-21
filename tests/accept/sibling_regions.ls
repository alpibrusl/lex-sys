// Two `region` blocks side by side — which crashed the compiler until
// `docs/benchmarks-game.md` §3.1.
//
// Arena numbers are handed out in the order the lowering meets the
// blocks, so these are 0 and 1. The backend kept open arenas on a
// *stack*, and the second block opens after the first has closed: the
// stack had length 0 where index 1 was wanted. In debug that tripped an
// assertion which had documented the wrong assumption since arenas
// landed; in release it indexed out of bounds.
//
// No program here had two regions side by side until a benchmark did,
// which is `porting.md`'s lesson once more: the bugs are in the programs
// nobody thought to write.
//~ STDOUT first 11
//~ STDOUT second 22
//~ STDOUT nested-then-sibling 7
//~ EXIT 0

import std.io;

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);

    var first = 0;
    var second = 0;
    var third = 0;

    region a {
        let s = alloc_slice[a](4, 11);
        first = s[0];
    }
    // `b` is arena 1, and `a` is already closed.
    region b {
        let s = alloc_slice[b](4, 22);
        second = s[0];
    }
    // A sibling of a *nested* pair, so the numbering skips: `c` is 2,
    // `d` is 3, and `e` is 4 with both of the others shut.
    region c {
        let outer = alloc_slice[c](2, 3);
        region d {
            let inner = alloc_slice[d](2, 4);
            third = outer[0] + inner[0];
        }
    }
    region e {
        let s = alloc_slice[e](1, 0);
        third = third + s[0];
    }

    borrow mut io as &!i in {
        io.write_all(i, "first ");
        io.print_int(i, first);
        io.newline(i);
        io.write_all(i, "second ");
        io.print_int(i, second);
        io.newline(i);
        io.write_all(i, "nested-then-sibling ");
        io.print_int(i, third);
        io.newline(i);
    }

    release(io);
    return 0;
}
