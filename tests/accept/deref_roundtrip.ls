// `docs/reading-references.md` §3: `*r` reads, `*r = v` writes, and the
// mode of the reference decides which is allowed.
//
// The second one is the more interesting: before this, a function could
// not hand a result back through a reference at all. `bump` below is an
// ordinary out-parameter, and it is only expressible because a whole value
// behind a reference can now be both read and replaced.
//~ STDOUT 41 42
//~ STDOUT 3 4 -> 30 40
//~ EXIT 0

fn print_nat[&i](io: &!i Io, n: int) -> [io] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, 48 + n % 10);
}

fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io] int {
    var n = 0;
    while n < len(s) {
        putchar(io, int_of(s[n]));
        n = n + 1;
    }
    return len(s);
}

// Reading through a shared reference. `&r int` has existed since M2 and
// this is the first thing that could be done with one.
fn peek[&r](n: &r int) -> [] int {
    return *n;
}

// Read-modify-write through a unique one.
fn bump[&r](n: &!r int) -> [] int {
    *n = *n + 1;
    return *n;
}

struct Point { x: int, y: int }

// A whole struct replaced through a reference, rather than field by
// field. `Point` is `val`, which is what makes the copy sound.
fn scale[&r](p: &!r Point, by: int) -> [] int {
    *p = Point { x: p.x * by, y: p.y * by };
    return p.x + p.y;
}

fn run[&i](io: &!i Io) -> [io] int {
    var n = 41;
    var before = 0;
    var after = 0;
    borrow n as &r in {
        before = peek(r);
    }
    borrow mut n as &!w in {
        after = bump(w);
    }
    print_nat(io, before);
    putchar(io, 32);
    print_nat(io, after);
    putchar(io, 10);

    var p = Point { x: 3, y: 4 };
    print_nat(io, p.x);
    putchar(io, 32);
    print_nat(io, p.y);
    write_all(io, " -> ");
    borrow mut p as &!q in {
        scale(q, 10);
    }
    // The write landed in `p` itself: the borrow ended and the value came
    // back.
    print_nat(io, p.x);
    putchar(io, 32);
    print_nat(io, p.y);
    putchar(io, 10);
    return p.x + p.y;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap } = split(world);
    release(heap);
    release(ffi);
    release(fs);

    var status = 0;
    borrow mut io as &!i in {
        status = run(i);
    }
    release(io);
    return status - 70;
}
