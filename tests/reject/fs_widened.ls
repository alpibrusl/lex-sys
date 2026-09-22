//~ ERROR cannot be narrowed to
//~ RULE capability-not-narrowable

// `docs/filesystem.md` §1: `Fs` narrows by the same prefix extension `Ffi`
// uses (§7.4), which means it narrows in one direction only.
//
// `effect_widened.ls` makes this point for libraries. It is worth making
// again for paths, because a path is the case where widening would be
// *useful* -- a program that has been given `/tmp/app` and wants `/tmp` is
// asking for the directory its siblings live in, and the answer is no.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // This program allocates nothing on the heap, so that authority ends here.
    release(heap);
    release(ffi);
    release(io);

    let app = narrow(fs, "/tmp/app");
    // Authority over one directory, asking to become authority over the
    // one above it.
    let wider = narrow(app, "/tmp");
    release(wider);
    return 0;
}
