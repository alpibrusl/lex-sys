//~ ERROR is declared `val`, but it holds

// `docs/sharing.md` §2.1, the third of three.
//
// Both earlier attempts were refused for handling the box wrongly. So
// stop handling it: declare `Rc` itself `val` and let the *language*
// copy it, the way it copies an `int`.
//
// This is refused before any function is checked, at the declaration --
// and it is the refusal that settles §2. A `val` type is duplicated by
// assignment, by passing, by returning; if one could hold a `Box`, every
// one of those would be a second owner, and the checker would have no
// event to see. So the rule is not about `Rc`: it is that `val` and `res`
// are what they say they are, everywhere, with no exception a library can
// buy.
//
// Three attempts, three rules, none of them written with `Rc` in mind.
// That is §2's claim: a copyable pointer is not a feature this language
// is missing, it is one the language is *made of* not having.

struct Cell {
    value: int,
    count: int,
}

val struct Rc {
    held: Box[Cell],
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
        // If the declaration above stood, this would be two owners of one
        // allocation written as an assignment, with nothing anywhere for
        // the checker to object to.
        let two = one;
        let Rc { held } = two;
        status = unbox(h, held).value;
    }
    release(heap);
    return status;
}
