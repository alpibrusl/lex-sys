//~ ERROR nothing moves out of a reference

// `docs/sharing.md` §2.1, the first of three.
//
// The obvious `Rc`: a struct holding a boxed cell, and a `clone` that
// takes a reference to one and builds a second struct pointing at the
// same cell. It is how every refcounting library in every other language
// begins, and here it does not get past the checker.
//
// The rule it hits was not written with `Rc` in mind. `reading-references`
// §2 says a reference gives references, so reading a `res` field through
// one would leave two owners of a single allocation -- which is the exact
// thing `clone` was trying to do. The refusal is not a technicality; it
// is the whole argument of §2 arriving one line early.

struct Cell {
    value: int,
    count: int,
}

res struct Rc {
    held: Box[Cell],
}

fn clone[&r](rc: &r Rc) -> [] Rc {
    // Two `Rc`s, one allocation. This is the line.
    return Rc { held: rc.held };
}

// End one `Rc`: take it apart and free the cell it holds.
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
        let one = Rc { held: box(h, Cell { value: 1, count: 1 }) };
        var copied = 0;
        borrow one as &r in {
            // If `clone` compiled, this would be a second owner of one
            // allocation -- and the `drain` below would free it again.
            // The double free is not caught here; it is unexpressible
            // three lines up.
            copied = drain(h, clone(r));
        }
        status = copied + drain(h, one);
    }
    release(heap);
    return status;
}
