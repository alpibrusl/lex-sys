//~ ERROR `Vec` bounds `T` by `val`, and `Ticket` is `res`

// `docs/collections.md` §3: the bound is kept where the type argument is
// supplied, which is the caller's own file rather than the library's.
//
// That placement is the whole argument for writing the bound on the
// declaration at all. Without it the run of elements would still be
// refused -- a boxed slice holds `val` data only -- but the refusal
// would land inside `empty`, pointing at a `box_slice` call the caller
// cannot change and did not write.

res struct Ticket {
    serial: int,
}

res struct Vec[T: val] {
    held: Box[[T]],
    used: int,
}

fn hold(v: Vec[Ticket]) -> [] int {
    return 0;
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
