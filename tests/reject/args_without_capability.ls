//~ ERROR expected `Args`, found `Io`
//~ RULE type-mismatch

// `docs/arguments.md` §2: reading the command line is reached through the
// capability that authorises it, and through nothing else.
//
// The argument for making this ambient is that arguments grant no *power*
// -- a program learns something, and learning is not authority. That is
// true and it is about containment. It does not decide the question,
// because an effect row here is about **visibility**: a function whose
// behaviour depends on the command line should say so in its type, and an
// ambient `arg` would let one eight frames down branch on `--force` with
// nothing in any signature saying that it does.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(ffi);
    release(fs);
    release(heap);
    release(args);

    var count = 0;
    borrow mut io as &!i in {
        // The console capability authorises writing to a terminal. It says
        // nothing about what the program was started with.
        count = arg_count(i);
    }
    release(io);
    return count;
}
