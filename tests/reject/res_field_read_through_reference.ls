//~ ERROR expected `Box[?0]`, found `&r Box[int]`

// `docs/reading-references.md` §2.0, and the fixture for a **soundness
// hole** that this rule has now closed twice.
//
// First it was allowed: a `res` field could be read through a shared
// reference, producing a second owner of a value the referent still
// owned. The program below compiled, and under valgrind it was an
// `Invalid free()` -- `steal` frees the box, `main` frees it again. Two
// obligations where one is owed, from ordinary code, with no `unsafe`
// anywhere, which this language does not have.
//
// Then it was refused outright, which overshot: a struct with a `res`
// field could not be read through a reference at all, while the
// equivalent enum could.
//
// Now `holder.held` is a `&r Box[int]` -- a borrow, exactly as `match`
// on a reference binds a `res` payload -- and this program is still
// refused, one line lower. That is the point of the fixture: the double
// free was never about *reading*, it was about *owning*, so the refusal
// belongs at the use, where the ordinary type rule already puts it. A
// borrow is not a `Box`, and no rule about field access is needed to say
// so.

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
