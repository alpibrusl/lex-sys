// `fib` -- recursion, so the cost is calls rather than arithmetic.
//
// Two checked subtractions and one checked addition per node, against a
// call, a return and a branch. Included because a language's arithmetic
// looks very different when the frame pointer is the bottleneck.
//
// This is the **wrapping** half of a pair: the same program with the
// traps removed, which is the baseline the checked half is measured
// against. It is not idiomatic lex-sys and is not meant to be —
// `wrapping_add` means *the bits are the intent* (`defined-behaviour.md`
// §2.2), and here the intent is only to delete the check.
//
// `python3 scripts/bench.py` runs the pair.

fn fib(n: int) -> [] int {
    if n < 2 {
        return n;
    }
    return wrapping_add(fib(wrapping_sub(n, 1)), fib(wrapping_sub(n, 2)));
}
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(io); release(ffi);
    return wrapping_sub(fib(32), 2178309);
}
