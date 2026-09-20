// Functions see each other regardless of definition order.
//~ STDOUT z
//~ EXIT 0

fn run[&i](io: &!i Io) -> [io] int {
    putchar(io, last_letter());
    putchar(io, 10);
    return 0;
}

fn last_letter() -> [] int {
    return 122;
}

fn main(world: World) -> [] int {
    // §8.2: the runtime hands over exactly one `World`, and `split` consumes
    // it. There is no other way to obtain a capability.
    let Split { io, ffi } = split(world);
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
