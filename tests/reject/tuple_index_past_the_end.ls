//~ ERROR they are numbered from 0

// `docs/tuples.md` §3.1: `t.2` on a two-tuple is not a field, and the
// arity is in the type, so this is a compile-time question rather than the
// runtime one `s[i]` asks. A tuple is not a slice: its length is part of
// what it *is*, which is why there is no bounds check to emit here.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);
    release(io);

    let pair = (1, 2);
    return pair.2;
}
