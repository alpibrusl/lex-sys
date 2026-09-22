//~ ERROR a reference may not outlive its region
//~ RULE reference-escapes-region

// `docs/reading-references.md` §2: a binding from a matched reference
// carries the scrutinee's *region*, so §5's occurs-check applies to it
// unchanged.
//
// That is the section's claim rather than a coincidence. No new escape
// rule was written for matching through a reference; the code that refuses
// `reference_escapes_arena.ls` refuses this, because the binding's type
// mentions `r` and `r` is a block in this function.

enum Pair {
    One(int),
    Two(int, int),
}

fn leak[&q](fallback: &q int) -> [] &q int {
    var p = Pair::One(7);
    borrow p as &r in {
        match r {
            Pair::One(a) => { return a; }
            Pair::Two(a, b) => { return b; }
        }
    }
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    return 0;
}
