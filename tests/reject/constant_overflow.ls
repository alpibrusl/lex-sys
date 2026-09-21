// `docs/compile-time.md` §4: a trap whose operands are in the source is
// certain, and a certainty is a diagnostic rather than a `SIGILL`.
//
// Before this slice the program below compiled cleanly and died when it
// ran. Nothing about the *guarantee* changed — `defined-behaviour.md`
// §2.1 always said an operation with no right answer stops — only when
// the programmer is told.
//~ ERROR this arithmetic overflows
//~ ERROR the operands are literals

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    let past_the_top = 9223372036854775807 + 1;
    return past_the_top;
}
