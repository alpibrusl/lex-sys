//~ ERROR expected `Box[?0]`, found `&p Box[int]`
//~ RULE type-mismatch

// `docs/tuples.md` §3.1, the reference case, and
// `reading-references.md` §2.0 in the place tuples add.
//
// A tuple is an anonymous struct, so it had better not need its own
// ideas about reading a component. `pair.0` through a reference is a
// `&p Box[int]` -- a borrow, because a `res` component cannot be copied
// -- exactly as `holder.held` is on a struct.
//
// *Binding* it is fine, and `tests/accept/borrowed_fields.ls` is that
// half. Consuming it is not, and that is this fixture: `unbox` takes a
// `Box`, a borrow is not one, and the double free the old rule was
// written to prevent is unexpressible without any rule about reading at
// all.
//
// A `val` component still copies, because copying one costs the referent
// nothing. `tests/accept/tuple_roundtrip.ls` is that half.

fn steal[&h, &p](heap: &!h Heap, pair: &p (Box[int], int)) -> [heap] int {
    let held = pair.0;
    return unbox(heap, held);
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(io);

    var status = 0;
    borrow mut heap as &!h in {
        let pair = (box(h, 41), 1);
        borrow pair as &p in {
            status = steal(h, p);
        }
        // The tuple still owns a box `steal` already freed.
        let (held, tag) = pair;
        status = status + unbox(h, held) + tag;
    }
    release(heap);
    return status;
}
