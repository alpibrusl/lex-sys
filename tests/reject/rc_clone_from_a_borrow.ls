//~ ERROR expected `Box[Cell]`, found `&r Box[Cell]`

// `docs/sharing.md` §2.1, the first of three.
//
// The obvious `Rc`: a struct holding a boxed cell, and a `clone` that
// takes a reference to one and builds a second struct pointing at the
// same cell. It is how every refcounting library in every other language
// begins, and here it does not get past the checker.
//
// The rule it hits was not written with `Rc` in mind, and it is now the
// plainest rule in the language rather than a special one about reading.
// `rc.held` through a reference is a `&r Box[Cell]`
// (`reading-references.md` §2.0) -- a borrow of the cell, not a second
// pointer to it -- and the struct literal wants a `Box[Cell]` it owns.
// A borrow is not an owner. That is the whole refusal, and it is the
// whole argument of `sharing.md` arriving one line early.

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
