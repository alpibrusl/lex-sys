// `reduce` -- a memory-fed reduction, which is the shape a GPU runs.
//
// `sum` is arithmetic with no memory traffic and bounds the answer from
// above; this is the loop `benches/reduce.c` already gives to clang, so
// the two languages can be compared on the same algorithm rather than on
// the same words. `docs/gpu.md` §2 is what the comparison is for: a GPU
// has no trap, so the only points reachable there are the wrapping ones,
// and the question is whether deleting the trap is enough to reach C.
//
// 200 rounds over a million elements.
//
// This is the **wrapping** half of a pair: the traps are deleted, which
// is what a GPU can actually run. Not idiomatic lex-sys and not meant
// to be -- `wrapping_add` means the bits are the intent, and here the
// intent is only to delete the check.

fn fill[&h](heap: &!h Heap, n: int) -> [heap] Box[[int]] {
    let held = box_slice(heap, n, 0);
    borrow mut held as &!b in {
        let v = contents(b);
        var i = 0;
        while i < n {
            // The same values `reduce.c` uses: -1, 0, 1 repeating, so
            // nothing can be constant-folded and nothing overflows.
            v[i] = i % 3 - 1;
            i = wrapping_add(i, 1);
        }
    }
    return held;
}

fn run[&b](v: &b [int], rounds: int) -> [] int {
    var total = 0;
    var r = 0;
    while r < rounds {
        var i = 0;
        while i < len(v) {
            total = wrapping_add(total, v[i]);
            i = wrapping_add(i, 1);
        }
        r = wrapping_add(r, 1);
    }
    return total;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(fs); release(io); release(ffi);

    var total = 0;
    borrow mut heap as &!h in {
        let held = fill(h, 1000000);
        borrow held as &b in {
            total = run(contents(b), 200);
        }
        unbox_slice(h, held);
    }
    release(heap);
    // -1, 0, 1 repeating over a million elements sums to -1 per round.
    return total - (0 - 200);
}
