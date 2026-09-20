// §5's happy path: a reference obtained, used and discarded inside its
// region, and the value owned again afterwards.
//
// The point is what a borrow makes possible that linearity alone did not.
// `tests/reject/res_field_read.ls` refuses `f.fd` on an owned `res` value,
// because reading a part out of one without taking it apart is a non-owning
// read -- and a non-owning read is exactly what this is.
//~ STDOUT 437
//~ EXIT 0

res struct File {
    fd: int,
    size: int,
}

fn open(fd: int) -> [] File {
    return File { fd: fd, size: fd * 2 };
}

fn close(f: File) -> [] int {
    let File { fd, size } = f;
    return fd + size;
}

// Region-polymorphic, with the region written (§5.1). The caller's region is
// substituted at the call site: one name, one assignment.
fn size_of[&r](handle: &r File) -> [] int {
    return handle.size;
}

fn run[&i](io: &!i Io) -> [io] int {
    let f = open(4);

    borrow f as &r in {
        // `r` is the region in a type and the reference in an expression.
        putchar(io, 48 + r.fd);
        putchar(io, 48 + size_of(r) / 2 - 1);
    }

    // Owned again: the block closed, so the freeze lifted.
    putchar(io, 48 + close(f) - 5);
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
