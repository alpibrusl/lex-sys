// Destructuring is §4.1's third consumer, and it works on any struct -- `res`
// or not. The whole is spent and the parts are produced, each subject to the
// rule in turn, so a `res` aggregate comes apart into fields that carry their
// own obligations.
//~ STDOUT 3412
//~ EXIT 0

struct Point {
    x: int,
    y: int,
}

// A `val` struct: destructuring it is just a convenience, since nothing was
// owed in the first place.
fn sum(p: Point) -> [] int {
    let Point { x, y } = p;
    return x + y;
}

res struct Ticket {
    fd: int,
}

// A `res` aggregate: `res` by inference, because a member is.
struct Pair {
    left: Ticket,
    right: Ticket,
}

fn close(f: Ticket) -> [] int {
    let Ticket { fd } = f;
    return fd;
}

// Both parts are live after the `let`, and both are owed.
fn close_both(p: Pair) -> [] int {
    let Pair { left, right } = p;
    return close(left) * 10 + close(right);
}

// Generic structs carry their argument's mode: `Held[Ticket]` is `res`,
// `Held[int]` is `val`, and neither needed a word written on it.
struct Held[T] {
    value: T,
}

fn unwrap[T](h: Held[T]) -> [] T {
    let Held { value } = h;
    return value;
}

fn run[&i](io: &!i Io) -> [io_write] int {
    putchar(io, 48 + sum(Point { x: 1, y: 2 }) - 0);
    putchar(io, 48 + unwrap(Held { value: 4 }));
    putchar(io, 48 + close_both(Pair { left: Ticket { fd: 1 }, right: Ticket { fd: 2 } }) / 10);
    putchar(io, 48 + close(unwrap(Held { value: Ticket { fd: 2 } })));
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
