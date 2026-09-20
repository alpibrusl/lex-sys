// `bool` is a type of its own in M1. A comparison yields one, `if` and `while`
// require one, and there is no conversion in either direction.
//~ STDOUT 1010010
//~ EXIT 0

fn digit(b: bool) -> [] int {
    if b {
        return 49;                   // '1'
    } else {
        return 48;                   // '0'
    }
}

fn run[&i](io: &!i Io) -> [io] int {
    putchar(io, digit(2 < 3));
    putchar(io, digit(3 <= 2));
    putchar(io, digit(4 == 4));
    putchar(io, digit(5 != 5));
    putchar(io, digit(true && false));
    putchar(io, digit(true || false));
    putchar(io, digit(!true));
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
