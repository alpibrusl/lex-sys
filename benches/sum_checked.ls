// `sum` -- the worst case, and it is meant to be.
//
// Three checked operations per iteration and nothing else: no memory
// traffic, no branches to predict, no calls. Arithmetic is the critical
// path, so this is the most the overflow check can possibly cost. Nobody
// writes this loop; it is here to bound the answer from above.
//
// 200 rounds of a million iterations.
//
// This is the **checked** half of a pair. Its twin differs only in using
// `wrapping_add`/`wrapping_sub`/`wrapping_mul`, which lower to a bare
// `iadd`/`isub`/`imul` with no trap — so the difference between the two
// binaries is exactly the cost of the guarantee, and nothing else.
//
// `python3 scripts/bench.py` runs the pair. Both return 0 when correct,
// which is how the suite checks that the two halves still agree.

fn run(rounds: int) -> [] int {
    var total = 0;
    var r = 0;
    while r < rounds {
        var i = 0;
        while i < 1000000 {
            total = total + i;
            total = total - i;
            i = i + 1;
        }
        r = r + 1;
    }
    return total;
}
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(io); release(ffi);
    return run(200);
}
