// What a checked effect row is worth, if anything could spend it.
//
// `slow_pure` is a pure function: its row is `[]` and it takes an `int`,
// so `lex-sys authority` reports it as provably pure. It is called twice
// with the same argument, two million times, and the answer is the same
// every time.
//
// A compiler that knew it was pure would call it **once**. Measured in C
// with `__attribute__((const))`, knowing is worth **159x** on this loop.
//
// lex-sys knows and cannot spend it: Cranelift has no way to say a call
// has no side effects, so every one of the four million calls happens.
// `docs/purity.md` §4 is what that costs and what would collect it.
//
// The other two are `purity.c` and `purity.rs`, and the point of the
// comparison is not the timing -- all three do the same work -- it is
// that of the three languages only this one can *state* the fact, and
// only C can act on it, unchecked.

import std.io;

// Expensive enough that a call is not noise, pure by construction.
fn slow_pure(x: int) -> [] int {
    var acc = 0;
    var i = 0;
    while i < 64 {
        acc = wrapping_add(wrapping_mul(acc, 31), x ^ i);
        i = i + 1;
    }
    return acc;
}

fn run(rounds: int) -> [] int {
    var total = 0;
    var i = 0;
    while i < rounds {
        total = wrapping_add(total, wrapping_add(slow_pure(7), slow_pure(7)));
        i = i + 1;
    }
    return total;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);

    let total = run(2000000);
    borrow mut io as &!i in {
        io.print_int(i, total);
        io.newline(i);
    }
    release(io);
    return 0;
}
