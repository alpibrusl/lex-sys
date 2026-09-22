//~ ERROR a reference may not outlive its region
//~ RULE reference-escapes-region

// `docs/heap.md` §3: `contents` is region-preserving, and that is the whole
// of its safety story.
//
// The reference it hands back carries the region of the borrow it came
// from, so §5's occurs-check applies to it unchanged -- the same code that
// refuses `reference_escapes_arena.ls` refuses this, and no new rule was
// written for the heap.
//
// The asymmetry is the point. A *box* outliving the block that made it is
// the entire feature: it is heap-allocated precisely so it can. A
// *reference into* a box outliving the borrow it was taken through is not,
// because the borrow is what promised the box would still be there.

struct Node {
    value: int,
}

fn escape_borrow[&q](b: Box[Node], fallback: &q Node) -> [] &q Node {
    borrow b as &r in {
        // `contents(r)` is `&r Node`. `r` is this block, and `q` is a
        // region the caller named, so no value mentioning `r` can leave.
        return contents(r);
    }
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    release(heap);
    release(ffi);
    release(fs);
    release(io);
    return 0;
}
