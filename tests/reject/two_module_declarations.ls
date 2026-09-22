//~ ERROR a file declares at most one module
//~ RULE program-shape

// `docs/modules.md` §3. A file is in one namespace.
//
// Two files may declare the *same* module and share it (§3.2) -- that is
// a module spanning files. One file declaring two modules is the other
// thing, and it has no meaning: the declarations after the first would
// have nothing to attach to but the items that follow them, which is a
// different feature (a nested module) with a different design.

module first;

module second;

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);
    release(io);
    return 0;
}
