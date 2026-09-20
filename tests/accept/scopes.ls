// An inner block may shadow; the outer binding is untouched.
//~ STDOUT ba
//~ EXIT 0

fn run[&i](io: &!i Io) -> [io] int {
    let x = 97;
    if true {
        let x = 98;
        putchar(io, x);
    }
    putchar(io, x);
    putchar(io, 10);
    return 0;
}

fn main(world: World) -> [] int {
    // §8.2: the runtime hands over exactly one `World`, and `split` consumes
    // it. There is no other way to obtain a capability.
    let Split { io, ffi, fs } = split(world);
    // This program touches no files, so that authority ends here.
    release(fs);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    var status = 0;
    // Threaded by borrow, not by move: a callee should not consume its
    // caller's authority.
    borrow mut io as &!i in {
        status = run(i);
    }
    // Authority is a resource, so it is destroyed exactly once. A program
    // that forgets this does not compile.
    release(io);
    return status;
}
