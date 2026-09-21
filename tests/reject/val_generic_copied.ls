//~ ERROR has already been consumed

// `docs/mode-polymorphism.md` §2, the worse half.
//
// A leak is bad. `val` also means **copyable**, and that is how the same
// hole gave a *double free*: two bindings of one `Wrap[Box[int]]`, each
// taken apart, each unboxing the same allocation.
//
// It compiled. Under valgrind: *Invalid free() / delete / delete[] /
// realloc().* The same shape as the double free in
// `docs/boxed-slices.md`, found the same way -- by testing a claim
// instead of believing it.
//
// Now `Wrap[Box[int]]` is `res`, because the mode is computed from the
// substituted members rather than read off the declaration. So the
// second binding is refused for the ordinary reason: a `res` value is
// used exactly once.

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
        let a = w;
        // One allocation, two owners, and two `unbox` calls below.
        let b = w;
        let Wrap { held } = a;
        let Wrap { held } = b;
        n = 0;
    }
    release(heap);
    return n;
}
