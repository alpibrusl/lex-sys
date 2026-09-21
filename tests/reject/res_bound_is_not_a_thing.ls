//~ ERROR there is no `res` bound

// `docs/mode-polymorphism.md` §3.2.
//
// `[T: res]` would mean "checked assuming `res`", which is exactly what
// an unbounded parameter already means. A bound that changes nothing is
// a keyword to explain and never reach for, so it does not exist -- and
// saying that in the refusal is better than accepting it and leaving
// someone to wonder what it bought them.

fn f[T: res](x: T) -> [] T {
    return x;
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
