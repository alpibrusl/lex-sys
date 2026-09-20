// §8.2 and §8.3: `main` splitting a `World` and threading `Io` down three
// frames. This is the thesis of the language in one program.
//
// Read the signatures rather than the bodies. `main` takes the `World` and
// its row is `[]` -- not because it does nothing, but because it *owns* the
// authority rather than borrowing it, and ownership is already visible in
// the parameter list (§8.2). Every frame below it borrows, and every one
// says `[io]` because a row lists what a function borrows.
//
// `digit` is the deepest frame and the only one that touches the console.
// Nothing here could print without being handed the capability, and there
// is nowhere else to get one.
//~ STDOUT 246
//~ EXIT 0

// Frame three: performs the effect.
fn digit[&i](io: &!i Io, n: int) -> [io] int {
    return putchar(io, 48 + n);
}

// Frame two: performs nothing itself, borrows on the way through.
fn pair[&i](io: &!i Io, a: int, b: int) -> [io] int {
    digit(io, a);
    return digit(io, b);
}

// Frame one.
fn run[&i](io: &!i Io) -> [io] int {
    pair(io, 2, 4);
    digit(io, 6);
    return putchar(io, 10);
}

// Pure arithmetic needs no capability and says so. A function with an empty
// row cannot print however much it wants to.
fn double(n: int) -> [] int {
    return n * 2;
}

fn main(world: World) -> [] int {
    // The one place authority enters a program. `split` consumes the
    // `World`: there is exactly one, and it is spent here.
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // This program touches no files, so that authority ends here.
    release(heap);
    release(fs);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);

    var status = 0;
    // Threaded by borrow, not by move -- a callee should not consume its
    // caller's authority.
    borrow mut io as &!i in {
        status = run(i) - 10 + double(0);
    }

    // Authority is a resource and is destroyed exactly once. Delete this
    // line and the program stops compiling.
    release(io);
    return status;
}
