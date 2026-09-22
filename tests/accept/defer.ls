// `docs/defer.md`: `defer E;` runs `E` at every exit from the block it
// is written in, in reverse order of declaration.
//
// The feature is sugar and stays sugar — expanded during lowering into
// the statement it stands for, once per exit path, so the linear
// checker replays exactly the events it would have replayed for the
// hand-written version. Nothing downstream knows `defer` exists, which
// is what keeps it from being a second set of linearity rules.
//
// §1 is the question this answers: is a consumption the programmer did
// not write at the point it happens still *visible*? Yes — the effect
// is still in the row, the exactly-once rule is still enforced on every
// path, and the line is still written, one below the acquisition. What
// moves is where the text sits, and it moves next to the thing it pairs
// with.
//~ STDOUT CBA
//~ STDOUT XYWZ
//~ STDOUT 012
//~ STDOUT 211
//~ EXIT 0

res struct Ticket {
    fd: int,
}

fn open(n: int) -> [] Ticket {
    return Ticket { fd: n };
}

fn close(f: Ticket) -> [] int {
    let Ticket { fd } = f;
    return fd;
}

// Reverse order of declaration: a later `defer` may depend on what an
// earlier one acquired, so the later one has to run first.
fn order[&i](io: &!i Io) -> [io_write] int {
    defer putchar(io, 65);
    defer putchar(io, 66);
    putchar(io, 67);
    return 0;
}

// Block scope, not function scope. `Y` belongs to the `if`, so it runs
// when that block closes rather than when the function does.
fn nested[&i](io: &!i Io) -> [io_write] int {
    defer putchar(io, 90);
    if true {
        defer putchar(io, 89);
        putchar(io, 88);
    }
    putchar(io, 87);
    return 0;
}

// A loop body is a block, so its frame runs once per iteration — which
// is what lets a resource be acquired and released inside one.
fn loops[&i](io: &!i Io) -> [io_write] int {
    var n = 0;
    while n < 3 {
        let f = open(n);
        defer close(f);
        putchar(io, 48 + n);
        n = n + 1;
    }
    return n;
}

// The shape §4.2 of `linearity-and-effects.md` calls verbose: one
// resource, several exits. A `return` runs every pending frame,
// innermost first, and the return *value* is evaluated before any of
// them — which is forced rather than chosen, since a `defer close(f)`
// alongside a `return` that reads `f` has to read it first.
fn take(flag: bool) -> [] int {
    let f = open(7);
    defer close(f);
    if flag {
        return 2;
    }
    return 4;
}

// Two frames at once: the `if`'s and the function body's, and the
// return runs both.
fn two_frames[&i](io: &!i Io, flag: bool) -> [io_write] int {
    let outer = open(1);
    defer putchar(io, 49);
    defer close(outer);
    if flag {
        let inner = open(2);
        defer putchar(io, 50);
        defer close(inner);
        return 10;
    }
    return 20;
}

fn run[&i](io: &!i Io) -> [io_write] int {
    let a = order(io);
    putchar(io, 10);
    let b = nested(io);
    putchar(io, 10);
    let c = loops(io);
    putchar(io, 10);
    let d = take(true) + take(false);
    // Innermost frame first: the `if`'s `2`, then the body's `1`.
    let e = two_frames(io, true) + two_frames(io, false);
    putchar(io, 10);
    return a + b + c + d + e;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    // This program touches no files, so that authority ends here.
    release(fs);
    release(heap);

    var status = 0;
    borrow mut io as &!i in {
        status = run(i);
    }
    release(io);
    return status - 39;
}
