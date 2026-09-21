//~ ERROR nothing moves out of a reference

// `docs/tuples.md` §3.1, the reference case, and
// `reading-references.md` §2 in the place tuples add.
//
// The rule is one sentence -- *nothing moves out of a reference, ever* --
// and the last time it was stated for one way of reaching into a value
// and not enforced for another, the gap was a double free reachable from
// ordinary code (`boxed-slices.md`, the `res` field through a reference).
// So this is a fixture written with the feature rather than after it.
//
// A `val` component still reads through a reference, because copying one
// costs the referent nothing. `tests/accept/tuple_roundtrip.ls` is that
// half.

fn peek[&p](pair: &p (Box[int], int)) -> [] int {
    let held = pair.0;
    return 0;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(io);

    var status = 0;
    borrow mut heap as &!h in {
        let pair = (box(h, 41), 1);
        borrow pair as &p in {
            status = peek(p);
        }
        let (held, tag) = pair;
        status = unbox(h, held) + tag;
    }
    release(heap);
    return status;
}
