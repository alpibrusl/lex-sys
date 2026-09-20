// Shared borrows nest (§5). Freezing is not exclusive: `frozen` says the
// value may not move or change, and two readers do neither.
//
// Both references are live at once and they have different regions, because
// each `borrow` block introduces its own. That costs nothing here -- each is
// used where its own region is expected.
//~ STDOUT 33
//~ EXIT 0

struct Bytes {
    len: int,
}

fn len_of[&r](b: &r Bytes) -> [] int {
    return b.len;
}

fn run[&i](io: &!i Io) -> [io] int {
    let buf = Bytes { len: 3 };

    borrow buf as &a in {
        borrow buf as &b in {
            putchar(io, 48 + len_of(a));
            putchar(io, 48 + len_of(b));
        }
    }

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
