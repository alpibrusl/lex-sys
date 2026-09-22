//~ ERROR a boxed slice holds `val` data only
//~ RULE mode-bound-violated

// `docs/collections.md` §2: the array-shaped collection cannot hold a
// resource, and the reason is not the generics.
//
// Two independent reasons, either of which alone would be enough. The
// fill is copied into every element, and a linear value cannot be
// copied at all -- that is the one this refusal names. The other is
// worse and is why no amount of API design gets around it:
// `unbox_slice` is one `free` that **runs nothing**, so an obligation
// inside the run would be dropped rather than discharged. There is no
// destructor to hang the discharge on, by design.
//
// The refusal lands here, at the definition, rather than at some
// instantiation -- an unbounded `[T]` is checked as though it were
// `res` (`docs/mode-polymorphism.md` §3.1), so this function can never
// be written, not merely never called with a resource. Writing
// `[T: val]` instead is what `std.vec` does, and then the refusal moves
// to the caller that asked for `Vec[Ticket]`.

res struct Vec[T] {
    held: Box[[T]],
    used: int,
}

fn empty[T, &h](heap: &!h Heap, capacity: int, fill: T) -> [heap] Vec[T] {
    return Vec { held: box_slice(heap, capacity, fill), used: 0 };
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);
    release(io);
    return 0;
}
