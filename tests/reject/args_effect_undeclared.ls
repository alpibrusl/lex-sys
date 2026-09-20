//~ ERROR does not declare

// §7.3 and `docs/arguments.md` §2: the row is exact, and reading argv is
// an effect like any other.
//
// This is the fixture that makes §2's claim mean something. `verbose`
// below changes what a program does based on the command line, and the
// whole point of the capability is that its row has to admit it.

fn verbose[&a](args: &a Args) -> [] bool {
    return arg_count(args) > 1;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(ffi);
    release(fs);
    release(heap);
    release(io);

    var loud = false;
    borrow args as &a in {
        loud = verbose(a);
    }
    release(args);
    if loud {
        return 1;
    }
    return 0;
}
