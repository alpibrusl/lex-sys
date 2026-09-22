//~ ERROR does not outlive
//~ RULE reference-escapes-region

// `docs/slicing.md` §3: a subslice is an **ordinary reference**, so it
// carries the region it was taken from and cannot outlive it.
//
// Nothing was added to the escape check for this. It is the same
// occurs-check over the same type that refuses `contents` escaping a
// borrow -- which is the point of the section: a subslice needed no rule
// about lifetimes because references already have them all.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(io);

    var escaped = "x";
    borrow mut heap as &!h in {
        let held = box_slice(h, 4, byte_of(65));
        borrow held as &r in {
            escaped = contents(r)[0..2];
        }
        unbox_slice(h, held);
    }
    release(heap);
    return len(escaped);
}
