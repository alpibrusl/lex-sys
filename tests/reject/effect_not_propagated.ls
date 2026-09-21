//~ ERROR performs `io_write`, which its row [] does not declare

// A row is transitive: `caller` performs whatever `shout` performs, because
// calling it is how the effect happens. This is what makes a row at the top
// of a program mean anything -- `main`'s row is the whole program's.
//
// This fixture carried a second, unintended error for a long time:
// `shout` was written `putchar(33)`, missing the `Io` that `putchar`
// takes. It passed anyway, because the checker happened to reach
// `caller` before `shout` and reported the effect error first. Making
// emission reachability-driven (`docs/standard-library.md` §5.2) also
// made checking run in source order, which surfaced it -- a fixture
// passing for the wrong reason is worth more attention than one failing.

fn shout[&i](io: &!i Io) -> [io_write] int {
    return putchar(io, 33);
}

fn caller[&i](io: &!i Io) -> [] int {
    return shout(io);
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
