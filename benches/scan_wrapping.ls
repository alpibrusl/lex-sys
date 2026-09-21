// `scan` -- counting words in a byte buffer.
//
// Comparisons, a boolean, a counter and an index: what text-handling
// systems code actually looks like. Two of the four operations per
// iteration are checked additions, and neither is what the processor is
// waiting for.
//
// This is the benchmark that came out backwards, which §3.3 of
// `docs/overflow-cost.md` is about.
//
// This is the **wrapping** half of a pair: the same program with the
// traps removed, which is the baseline the checked half is measured
// against. It is not idiomatic lex-sys and is not meant to be —
// `wrapping_add` means *the bits are the intent* (`defined-behaviour.md`
// §2.2), and here the intent is only to delete the check.
//
// `python3 scripts/bench.py` runs the pair.

fn words[&b](text: &b [byte]) -> [] int {
    var count = 0;
    var inside = false;
    var i = 0;
    while i < len(text) {
        let c = int_of(text[i]);
        if c == 32 || c == 10 {
            inside = false;
        } else {
            if !inside {
                count = wrapping_add(count, 1);
            }
            inside = true;
        }
        i = wrapping_add(i, 1);
    }
    return count;
}
fn run(rounds: int) -> [] int {
    var total = 0;
    var r = 0;
    region a {
        let text = alloc_slice[a](60000, byte_of(97));
        var k = 0;
        while k < 60000 {
            if wrapping_sub(k, wrapping_mul(k / 7, 7)) == 0 {
                text[k] = byte_of(32);
            }
            k = wrapping_add(k, 1);
        }
        while r < rounds {
            total = words(text);
            r = wrapping_add(r, 1);
        }
    }
    return wrapping_sub(total, 8572);
}
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(io); release(ffi);
    return run(2000);
}
