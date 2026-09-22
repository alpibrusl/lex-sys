//~ ERROR destructuring takes the whole value apart
//~ RULE arity-mismatch

// `docs/tuples.md` §3.2. The same rule a struct pattern has, and the same
// reason: destructuring is the operation that *discharges* a linear
// obligation, so a pattern that could leave a component unnamed would be a
// way to drop one.
//
// The components here are `val`, so nothing would actually leak. The rule
// does not ask -- a pattern that means one thing for `val` and another for
// `res` is two rules, and the whole argument for tuples is that they need
// no rules of their own.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);
    release(io);

    let three = (1, 2, 3);
    let (a, b) = three;
    return a + b;
}
