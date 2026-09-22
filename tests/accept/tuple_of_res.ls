//~ STDOUT 41 and 1
//~ EXIT 0

// `docs/tuples.md` §2.3, the whole of it in one program: a tuple holding
// a `Box` is `res`, with nothing in the source saying so.
//
// It is threaded through a function, returned, destructured and ended
// exactly like a `res struct` would be -- which is the claim. The mode is
// computed from the components because there is no declaration site to
// write one at, and the obligations that follow are not weaker for having
// been computed.
//
// `(Box[int], int)` is two leaves, so this one comes back in registers.
// `examples/slab/` is the other case: five leaves, through memory, the
// way the `res struct` it replaced did (§5).

fn hold[&h](heap: &!h Heap, n: int) -> [heap] (Box[int], int) {
    return (box(heap, n), 1);
}

fn print_nat[&i](io: &!i Io, n: int) -> [io_write] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, '0' + n % 10);
}

fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io_write] int {
    var n = 0;
    while n < len(s) {
        putchar(io, int_of(s[n]));
        n = n + 1;
    }
    return len(s);
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);

    var value = 0;
    var tag = 0;
    borrow mut heap as &!h in {
        let held = hold(h, 41);
        let (boxed, marker) = held;
        value = unbox(h, boxed);
        tag = marker;
    }
    release(heap);

    borrow mut io as &!i in {
        print_nat(i, value);
        write_all(i, " and ");
        print_nat(i, tag);
        putchar(i, 10);
    }
    release(io);
    return value - 41;
}
