// Linearity's happy path (§4): a `res` value is created, threaded through a
// function that hands it back, and finally taken apart. Destructuring to
// `val` parts is where the obligation ends -- there is no `drop`, so a
// resource is destroyed by the function that knows how.
//~ STDOUT 79
//~ EXIT 0

res struct Ticket {
    fd: int,
}

fn open(fd: int) -> [] Ticket {
    return Ticket { fd: fd };
}

// Takes ownership and gives it back: the caller still owes one consumption.
fn touch(f: Ticket) -> [] Ticket {
    let Ticket { fd } = f;
    return Ticket { fd: fd + 2 };
}

// The terminal consumer. The parts are `int`, which is `val`, so nothing is
// owed once they are out.
fn close(f: Ticket) -> [] int {
    let Ticket { fd } = f;
    return fd;
}

fn run[&i](io: &!i Io) -> [io_write] int {
    let straight = open(7);
    putchar(io, 48 + close(straight));

    let threaded = open(7);
    putchar(io, 48 + close(touch(threaded)));

    putchar(io, 10);
    return 0;
}

fn main(world: World) -> [] int {
    // §8.2: the runtime hands over exactly one `World`, and `split` consumes
    // it. There is no other way to obtain a capability.
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // This program touches no files, so that authority ends here.
    release(heap);
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
