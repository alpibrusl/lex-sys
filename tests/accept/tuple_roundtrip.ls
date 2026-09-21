//~ STDOUT 7 true
//~ STDOUT 42

// `docs/tuples.md` §3, the `val` half: construct, return, destructure,
// `.0` on an owner, and `.0` through a reference.
//
// The last of those is the one worth naming. `sum` reads both components
// of a tuple it does not own, and that is legal precisely because they
// are `val`: a copy costs the referent nothing, so there is no second
// owner and nothing for `reading-references.md` §2 to object to. The
// `res` side of the same line is `tests/reject/`'s
// `res_tuple_component_through_reference.ls`.

fn swap(pair: (int, bool)) -> [] (bool, int) {
    let (number, flag) = pair;
    return (flag, number);
}

fn sum[&p](pair: &p (int, int)) -> [] int {
    return pair.0 + pair.1;
}

fn print_nat[&i](io: &!i Io, n: int) -> [io_write] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, 48 + n % 10);
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
    release(heap);

    var status = 0;
    borrow mut io as &!i in {
        let flipped = swap((7, true));
        print_nat(i, flipped.1);
        putchar(i, 32);
        if flipped.0 {
            write_all(i, "true");
        } else {
            write_all(i, "false");
        }
        putchar(i, 10);

        let pair = (40, 2);
        var total = 0;
        borrow pair as &p in {
            total = sum(p);
        }
        print_nat(i, total);
        putchar(i, 10);
        status = total - 42;
    }
    release(io);
    return status;
}
