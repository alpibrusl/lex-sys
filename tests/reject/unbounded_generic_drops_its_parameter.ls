//~ ERROR is still live here
//~ RULE linear-value-unconsumed

// `docs/mode-polymorphism.md` §3.1 and §4.
//
// An **unbounded** type parameter is checked as though it were `res` --
// the stronger obligation -- so a body that passes the check is safe at
// every instantiation. `sink` drops its `x`, which is only legal for a
// copyable type, so it is refused **here**, where it is written.
//
// This fixture used to be `res_leaked_from_generic.ls` and asserted the
// opposite: a parameter was `val`, the body was accepted where it was
// written, and monomorphisation refused the copy at `Ticket` with
// "(instantiated at `Ticket`)". That is the behaviour §12 of
// `linearity-and-effects.md` wanted moved, and this is it moved.
//
// The trade is the right way round. A library ships its definitions, and
// an error inside `std/` pointing at code the caller cannot change is
// the worst possible place for it. A function that meant only copyable
// types says `[T: val]`, and then the refusal lands on the call site --
// `val_bound_violated_at_the_call_site.ls` is that half.

res struct Ticket {
    fd: int,
}

fn sink[T](x: T) -> [] int {
    return 0;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);
    release(io);
    return sink(Ticket { fd: 5 });
}
