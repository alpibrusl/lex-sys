//~ ERROR shadowing it here would put that value out of reach
//~ RULE linear-value-unconsumed

// `docs/shadowing.md` §4.1.
//
// This program was always refused. What changed is *where*: the value
// stayed live to the end of the function, so the old message landed on
// `return t;` -- a line where `t` is an `int` and the complaint was
// about a binding the line does not mention.
//
// A parameter is the one place two scopes are really one. The parameter
// list has no statements of its own and closes with the body's top-level
// block, so a `let` at the top of a body covers a parameter for the
// whole of that parameter's life. Which makes it a strand, not a shadow,
// and it is reported at the `let`.

res struct Ticket {
    serial: int,
}

fn redeem(t: Ticket) -> [] int {
    let t = 1;
    return t;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);
    release(io);

    return redeem(Ticket { serial: 1 });
}
