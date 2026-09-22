//~ ERROR a component cannot be read out of it
//~ RULE linear-value-taken-apart

// `docs/tuples.md` §3.1, the owner case.
//
// Reading one component out of a `res` tuple would consume the whole
// value -- that is what reading an aggregate is -- and leave the other
// components owed by nobody. Here that is a `Box[int]` that never reaches
// an `unbox`, which is a leak rather than a double free, and refused for
// the same reason: an obligation this language created and then dropped.
//
// The fix is the line below it: take the whole thing apart, which names
// both components and puts each under the rule in turn.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(io);

    var status = 0;
    borrow mut heap as &!h in {
        let pair = (box(h, 41), 1);
        // `pair.1` is an `int`, and reading it still consumes the tuple.
        status = pair.1;
        let (held, tag) = pair;
        status = unbox(h, held) + tag;
    }
    release(heap);
    return status;
}
