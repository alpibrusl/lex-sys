//~ ERROR a `val` declaration already bounds its parameters

// `docs/collections.md` §3.
//
// A bound on a type declaration is written where it is not already
// implied, and a `val` aggregate implies one on every parameter: `val`
// is a claim about *every* instantiation, and it can only hold if every
// argument is `val` too. So `val struct Wrap[T]` already **is**
// `val struct Wrap[T: val]`, and writing it out is a second way to say
// one thing.
//
// A `res` or undeclared aggregate implies nothing of the kind, which is
// why `res struct Vec[T: val]` is the shape that needed this.

val struct Wrap[T: val] {
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
