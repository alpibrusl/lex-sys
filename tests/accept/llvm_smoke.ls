// The LLVM backend's first slice (`docs/llvm-backend.md` §5): the smallest
// program that exercises the doorway -- function declarations and calls,
// `World`/capability erasure, `putchar`, and process exit. No arithmetic,
// no traps, no aggregates with real fields, no strings.
//
// `examples/hello.ls` was the original candidate (`docs/llvm-backend.md`
// §5, before this fixture's PR corrected it) but turns out to need checked
// arithmetic, bounds-checked indexing and string-literal data -- none of
// which this slice lowers. This fixture is what the doc's original bullet
// list actually describes, and `crates/lex-sys-codegen-llvm`'s own test
// builds it through `--backend llvm` and checks its output against the
// Cranelift path's, which this file's ordinary place in `tests/accept/`
// already checks against the directives below.
//~ STDOUT Hi!
//~ EXIT 0

fn greet[&i](io: &!i Io) -> [io_write] int {
    putchar(io, 72);
    putchar(io, 105);
    putchar(io, 33);
    return putchar(io, 10);
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // This program touches no files, so that authority ends here.
    release(heap);
    release(fs);
    // Nothing here calls into C directly, so that authority is dropped at once.
    release(ffi);

    borrow mut io as &!i in {
        greet(i);
    }
    release(io);
    return 0;
}
