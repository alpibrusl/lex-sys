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

res struct File {
    fd: int,
}

// A `res` aggregate: `res` by inference, because a member is.
struct Pair {
    left: File,
    right: File,
}

fn close(f: File) -> [] int {
    let File { fd } = f;
    return fd;
}

// Both parts are live after the `let`, and both are owed.
fn close_both(p: Pair) -> [] int {
    let Pair { left, right } = p;
    return close(left) * 10 + close(right);
}

// Generic structs carry their argument's mode: `Held[File]` is `res`,
// `Held[int]` is `val`, and neither needed a word written on it.
struct Held[T] {
    value: T,
}

fn unwrap[T](h: Held[T]) -> [] T {
    let Held { value } = h;
    return value;
}

fn main() -> [io] int {
    putchar(48 + sum(Point { x: 1, y: 2 }) - 0);
    putchar(48 + unwrap(Held { value: 4 }));
    putchar(48 + close_both(Pair { left: File { fd: 1 }, right: File { fd: 2 } }) / 10);
    putchar(48 + close(unwrap(Held { value: File { fd: 2 } })));
    putchar(10);
    return 0;
}
