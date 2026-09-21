// `fib` -- recursion, so the cost is calls rather than arithmetic.
//
// Two checked subtractions and one checked addition per node, against a
// call, a return and a branch. Included because a language's arithmetic
// looks very different when the frame pointer is the bottleneck.
//
// This is the **checked** half of a pair. Its twin differs only in using
// `wrapping_add`/`wrapping_sub`/`wrapping_mul`, which lower to a bare
// `iadd`/`isub`/`imul` with no trap — so the difference between the two
// binaries is exactly the cost of the guarantee, and nothing else.
//
// `python3 scripts/bench.py` runs the pair. Both return 0 when correct,
// which is how the suite checks that the two halves still agree.

fn fib(n: int) -> [] int {
    if n < 2 {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(io); release(ffi);
    return fib(32) - 2178309;
}
