// Precedence, associativity, signed division and remainder.
//~ STDOUT 7531
//~ EXIT 0

fn run[&i](io: &!i Io) -> [io] int {
    putchar(io, 48 + 1 + 2 * 3);         // precedence: 7
    putchar(io, 48 + (10 - 3 - 2));      // left-associative: 5
    putchar(io, 48 + (-6 / 2 + 6));      // truncating division: 3
    putchar(io, 48 + 7 % 3);             // remainder: 1
    putchar(io, 10);
    return 0;
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
