//~ ERROR shadowing it here would put that value out of reach
//~ RULE linear-value-unconsumed

// `docs/shadowing.md` §3.3, and the hazard the blanket refusal existed
// for.
//
// Nothing could ever `unbox` the first allocation: the name is the only
// way to reach it, and the second `let` takes the name. That is a leak,
// and a leak is exactly the case §4 of `linearity-and-effects.md` says
// affine types drop silently and this one does not.
//
// The rule that refuses it is not new. `assign_over_live_res.ls` is the
// same rule with `held = box(h, 2);` instead -- one rule, two syntaxes,
// and until this slice only one of them said so.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(io);

    var status = 0;
    borrow mut heap as &!h in {
        let held = box(h, 1);
        // The first allocation is now unreachable and still owed.
        let held = box(h, 2);
        status = unbox(h, held);
    }
    release(heap);
    return status;
}
