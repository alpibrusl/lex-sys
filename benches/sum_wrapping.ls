// `sum` -- the worst case, and it is meant to be.
//
// Three checked operations per iteration and nothing else: no memory
// traffic, no branches to predict, no calls. Arithmetic is the critical
// path, so this is the most the overflow check can possibly cost. Nobody
// writes this loop; it is here to bound the answer from above.
//
// 200 rounds of a million iterations.
//
// This is the **wrapping** half of a pair: the same program with the
// traps removed, which is the baseline the checked half is measured
// against. It is not idiomatic lex-sys and is not meant to be —
// `wrapping_add` means *the bits are the intent* (`defined-behaviour.md`
// §2.2), and here the intent is only to delete the check.
//
// `python3 scripts/bench.py` runs the pair.

fn run(rounds: int) -> [] int {
    var total = 0;
    var r = 0;
    while r < rounds {
        var i = 0;
        while i < 1000000 {
            total = wrapping_add(total, i);
            total = wrapping_sub(total, i);
            i = wrapping_add(i, 1);
        }
        r = wrapping_add(r, 1);
    }
    return total;
}
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(io); release(ffi);
    return run(200);
}
