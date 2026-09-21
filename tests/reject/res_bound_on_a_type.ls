//~ ERROR there is no `res` bound

// `docs/collections.md` §3, and the same rule `res_bound_is_not_a_thing`
// makes for a function: an unbounded parameter is already checked as
// `res`, so a `res` bound would change nothing.
//
// Both, because the two are parsed in one place and a rule that holds in
// one position and not the other is the kind of thing a refactor takes
// away quietly.

res struct Holder[T: res] {
    held: T,
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);
    release(io);
    return 0;
}
