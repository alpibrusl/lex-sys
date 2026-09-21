//~ ERROR shadowing it here would put that value out of reach

// `docs/shadowing.md` §3, through a destructuring pattern rather than a
// plain `let`.
//
// Worth its own fixture because the check lives in `declare`, which every
// binding form goes through -- a `let`, a struct pattern, a tuple pattern
// and a `match` arm alike. A rule enforced in one binding form and not
// the others is how the double free in #26 happened, so this is the
// second form asserted rather than assumed.

res struct Pair {
    left: Box[int],
    right: int,
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(io);

    var status = 0;
    borrow mut heap as &!h in {
        let first = Pair { left: box(h, 1), right: 1 };
        let second = Pair { left: box(h, 2), right: 2 };
        let Pair { left, right } = first;
        // `left` still holds the first allocation.
        let Pair { left, right } = second;
        status = unbox(h, left) + right;
    }
    release(heap);
    return status;
}
