// §5 rule 2: `borrow mut` locks the value for the block and binds a `&!r`
// reference that may be written through. Nothing else may touch the value
// while it is locked -- not a read, not a second borrow, not a move -- which
// is what makes the reference the only way to reach it.
//
// The last digit is the point: the write went through the reference and the
// owner sees it afterwards. Under the hood the value is spilled to a buffer
// for the block and read back when it closes, which is sound precisely
// because the lock meant nothing else could have moved on.
//~ STDOUT 3557
//~ EXIT 0

struct Counter {
    n: int,
    step: int,
}

// Region-polymorphic over a unique reference, and it writes through it.
fn bump[&r](c: &!r Counter) -> [] int {
    c.n = c.n + c.step;
    return c.n;
}

fn run[&i](io: &!i Io) -> [io_write] int {
    var c = Counter { n: 1, step: 2 };

    borrow mut c as &!r in {
        putchar(io, '0' + bump(r));
        putchar(io, '0' + bump(r));
    }

    // Owned again, and carrying what the reference wrote.
    putchar(io, '0' + c.n);

    // `&!r` is `val`, so it copies -- and the copies are copies of one
    // *pointer*, so writes through them alias rather than racing to be the
    // last writer. Copying the value in and out instead would print 4 here:
    // `b` would never have seen `a`'s write.
    var d = Counter { n: 0, step: 0 };
    borrow mut d as &!r in {
        let a = r;
        let b = r;
        a.n = 3;
        b.n = b.n + 4;
    }
    putchar(io, '0' + d.n);

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
