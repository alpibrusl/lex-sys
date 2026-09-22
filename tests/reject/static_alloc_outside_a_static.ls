// `docs/compile-time-data.md` §2.1: `alloc_slice[static]` is the one way
// to put something new in the static region, and it is **lexical** to a
// `static` item.
//
// A reachability rule — "legal in anything only a `static` calls" — would
// make the diagnostic depend on what the whole program reaches, which is
// what §4.1 of `compile-time.md` argues against for the same reason.
//~ ERROR only legal inside a `static` item
//~ RULE reference-escapes-region

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    let table = alloc_slice[static](4, 0);
    return len(table);
}
