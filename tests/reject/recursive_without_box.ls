//~ ERROR contains itself
//~ ERROR put a `Box` on the path back to it

// `docs/heap.md` §4: the size check has exactly one hole in it, and a type
// that contains itself *directly* is not in it.
//
// `recursive_struct.ls` made this point when there was no indirection at
// all and the answer was "not yet". The answer now is "not like that" --
// the error names the fix, because from M3 there is one.

enum Tree {
    Leaf,
    Node(Tree, int, Tree),
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    release(heap);
    release(ffi);
    release(fs);
    release(io);
    return 0;
}
