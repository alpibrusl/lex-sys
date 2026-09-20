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

fn main() -> [io] int {
    let f = open(4);

    borrow f as &r in {
        // `r` is the region in a type and the reference in an expression.
        putchar(48 + r.fd);
        putchar(48 + size_of(r) / 2 - 1);
    }

    // Owned again: the block closed, so the freeze lifted.
    putchar(48 + close(f) - 5);
    putchar(10);
    return 0;
}
