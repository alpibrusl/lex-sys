//~ ERROR a boxed slice holds `val` data only
//~ RULE mode-bound-violated

// `docs/boxed-slices.md` §2.1: the same rule an arena has (§6.1), for the
// same reason, stated once and enforced in both places.
//
// Ending a boxed slice frees memory and **runs nothing**, so a linear
// obligation put inside one would be dropped rather than discharged. A
// slice makes it worse than a single box would -- there would be `count`
// of them -- but the rule is the one rule, not a new one.
//
// And the fill is *copied* into every element, which a `res` value cannot
// be at all: that is what `res` means.

res struct Ticket { serial: int }

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(io);

    var n = 0;
    borrow mut heap as &!h in {
        let many = box_slice(h, 4, Ticket { serial: 1 });
        n = unbox_slice(h, many);
    }
    release(heap);
    return n;
}
