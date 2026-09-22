//~ ERROR needs `T` to be `val`, and `Ticket` is `res`
//~ RULE mode-bound-violated

// `docs/mode-polymorphism.md` §3.1 — the half §12 said was missing: *a
// signature that says so and is checked once*.
//
// `sink` declares `[T: val]`, so it is checked once at a copyable `T`
// and accepted: dropping `x` is fine for a type with no obligation. The
// promise is then kept at the **call site**, which is where the mistake
// is -- the caller chose `Ticket`, the callee did not.
//
// Compare `unbounded_generic_drops_its_parameter.ls`, which is the same
// body without the bound and is refused at the definition instead. Two
// fixtures for one rule, because the rule is about *where* the error
// goes and one of them could not show that alone.

res struct Ticket {
    fd: int,
}

fn sink[T: val](x: T) -> [] int {
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
