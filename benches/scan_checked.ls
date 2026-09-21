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
// This is the **checked** half of a pair. Its twin differs only in using
// `wrapping_add`/`wrapping_sub`/`wrapping_mul`, which lower to a bare
// `iadd`/`isub`/`imul` with no trap — so the difference between the two
// binaries is exactly the cost of the guarantee, and nothing else.
//
// `python3 scripts/bench.py` runs the pair. Both return 0 when correct,
// which is how the suite checks that the two halves still agree.

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
                count = count + 1;
            }
            inside = true;
        }
        i = i + 1;
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
            if k - (k / 7) * 7 == 0 {
                text[k] = byte_of(32);
            }
            k = k + 1;
        }
        while r < rounds {
            total = words(text);
            r = r + 1;
        }
    }
    return total - 8572;
}
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(io); release(ffi);
    return run(2000);
}
