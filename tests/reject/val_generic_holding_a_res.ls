//~ ERROR is still live at the end of this block
//~ RULE linear-value-unconsumed

// `docs/mode-polymorphism.md` §2 — the leak, and the reason this slice
// exists.
//
// `val struct Wrap[T]` is a promise about **every** `T`, and it used to
// be believed rather than checked: `mode_of` returned the declared mode
// without substituting the argument, so `Wrap[Box[int]]` was `val` by
// assertion. A `val` value is discardable, so nothing had to consume
// this one and the allocation was simply lost.
//
// It compiled. Under valgrind: *8 bytes in 1 blocks are definitely
// lost.* Ordinary code, no `unsafe` anywhere, which this language does
// not have.
//
// The declaration-time check that refuses "`X` is declared `val`, but it
// holds ..., which is `res`" could not catch it either, because it runs
// against the members **as written**, where `T` is a parameter rather
// than `Box[int]`.
//
// The fix is §3: declaring the aggregate `val` is a bound on its own
// parameters, kept where the argument is known. The mode is also
// computed honestly now rather than trusted -- which is what catches
// *this* program, where the instantiation is inferred from a struct
// literal and never written down anywhere for a check to look at.

val struct Wrap[T] {
    held: T,
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(io);

    var n = 0;
    borrow mut heap as &!h in {
        let w = Wrap { held: box(h, 41) };
        n = 1;
    }
    release(heap);
    return n - 1;
}
