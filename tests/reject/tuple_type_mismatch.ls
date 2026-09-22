//~ ERROR expected `int`, found `bool`
//~ RULE type-mismatch

// `docs/tuples.md` §2.2: a tuple is structural, so `(int, bool)` and
// `(bool, int)` are two types and the components are matched in order.
//
// This is the one place structural typing could have been loose and is
// not. Unification walks the components pairwise and arity is part of the
// match, so there is no prefix rule, no coercion and no reordering: the
// only tuple that unifies with `(int, bool)` is one written the same way.

fn flip(pair: (int, bool)) -> [] (int, bool) {
    return pair;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);
    release(io);

    let flipped = flip((true, 1));
    return flipped.0;
}
