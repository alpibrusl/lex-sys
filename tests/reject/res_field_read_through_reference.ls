//~ ERROR nothing moves out of a reference

// `docs/reading-references.md` §2, and the fixture for a **soundness
// hole** this rule was already supposed to close.
//
// §2 says "nothing moves out of a reference, ever". That was stated for
// `match` and enforced there. Field access is the other way to reach into
// a value, and until this fixture existed it was not enforced here: a
// `res` field could be read through a shared reference, producing a second
// owner of a value the referent still owned.
//
// It was not theoretical. The program below compiled, and under valgrind
// it was an `Invalid free()`: `steal` frees the box, `main` frees it
// again. Two obligations where one is owed, from ordinary code, with no
// `unsafe` anywhere -- which this language does not have.
//
// A `val` field is still read through a reference, because copying one
// costs the referent nothing. A `res` field cannot be copied, which is
// what `res` means.

res struct Holder {
    held: Box[int],
}

fn steal[&h, &r](heap: &!h Heap, holder: &r Holder) -> [heap] int {
    let taken = holder.held;
    return unbox(heap, taken);
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(fs);
    release(ffi);
    release(io);

    var n = 0;
    borrow mut heap as &!h in {
        let holder = Holder { held: box(h, 41) };
        borrow holder as &r in {
            n = steal(h, r);
        }
        // The holder still owns a box `steal` already freed.
        let Holder { held } = holder;
        n = n + unbox(h, held);
    }
    release(heap);
    return n - 82;
}
