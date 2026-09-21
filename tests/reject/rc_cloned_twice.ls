//~ ERROR used exactly once

// `docs/sharing.md` §2.1, the second of three.
//
// §2.1's first fixture is refused because `clone` read the cell through a
// *reference*. So try the other way: consume the `Rc` outright and hand
// back two. Nothing is read through a reference here, and the function
// owns what it is taking apart -- so the rule that refused the first
// attempt has nothing to say about this one.
//
// It is refused anyway, by linearity itself: the first `rc.held` moves the
// box out, and the second asks for a value that is already gone. That is
// the same rule that makes a double free unexpressible, arriving at the
// same conclusion from the opposite direction.
//
// Between them the two fixtures close the shape: whether `clone` borrows
// or consumes, one allocation cannot end up with two owners.

struct Cell {
    value: int,
    count: int,
}

res struct Rc {
    held: Box[Cell],
}

// No tuples (`docs/sharing.md` §4), so the pair of handles needs a name.
res struct Two {
    first: Rc,
    second: Rc,
}

fn clone(rc: Rc) -> [] Two {
    let Rc { held } = rc;
    // The first of these moves the box. The second is the line.
    return Two { first: Rc { held: held }, second: Rc { held: held } };
}

fn drain[&h](heap: &!h Heap, rc: Rc) -> [heap] int {
    let Rc { held } = rc;
    return unbox(heap, held).value;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(io);

    var status = 0;
    borrow mut heap as &!h in {
        let one = Rc { held: box(h, Cell { value: 1, count: 2 }) };
        let Two { first, second } = clone(one);
        status = drain(h, first) + drain(h, second);
    }
    release(heap);
    return status;
}
