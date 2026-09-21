// Enums with payloads, matched exhaustively, including a struct inside a
// payload and a `_` arm. An enum is a tag plus every variant's payload in M1 --
// overlaying them would be a layout decision, and M1 makes none.
//~ STDOUT 001220069901
//~ EXIT 0

struct Point { x: int, y: int }

enum Shape {
    Empty,
    Circle(int),
    Rect(int, int),
    At(Point, int),
}

fn area(s: Shape) -> [] int {
    match s {
        Shape::Empty => { return 0; }
        Shape::Circle(r) => { return 3 * r * r; }
        Shape::Rect(w, h) => { return w * h; }
        Shape::At(p, r) => { return p.x + p.y + r; }
    }
}

fn describe(s: Shape) -> [] int {
    match s {
        Shape::Circle(_) => { return 99; }
        _ => { return 1; }
    }
}

fn digits[&i](io: &!i Io, n: int) -> [io_write] int {
    putchar(io, 48 + n / 10);
    putchar(io, 48 + n % 10);
    return n;
}

fn run[&i](io: &!i Io) -> [io_write] int {
    digits(io, area(Shape::Empty));            // 00
    digits(io, area(Shape::Circle(2)));        // 12
    digits(io, area(Shape::Rect(4, 5)));       // 20
    digits(io, area(Shape::At(Point { x: 1, y: 2 }, 3)));  // 06
    digits(io, describe(Shape::Circle(1)));    // 99
    digits(io, describe(Shape::Empty));        // 01
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
