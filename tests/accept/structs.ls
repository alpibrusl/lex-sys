// Structs: nested values, passed and returned by value, with field access
// chains. A struct has no layout in M1 -- it is scalarised into its leaf
// fields -- so nothing here depends on how one would be laid out in memory.
//~ STDOUT 18.
//~ EXIT 0

struct Point { x: int, y: int }
struct Line { from: Point, to: Point, dashed: bool }

fn length_squared(l: Line) -> [] int {
    let dx = l.to.x - l.from.x;
    let dy = l.to.y - l.from.y;
    return dx * dx + dy * dy;
}

fn shift(p: Point, by: int) -> [] Point {
    return Point { x: p.x + by, y: p.y + by };
}

fn run[&i](io: &!i Io) -> [io_write] int {
    let a = Point { x: 1, y: 2 };
    let b = shift(a, 3);
    let l = Line { from: a, to: b, dashed: false };
    // (4-1)^2 + (5-2)^2 = 9 + 9 = 18
    putchar(io, '0' + length_squared(l) / 10);
    putchar(io, '0' + length_squared(l) % 10);
    if l.dashed {
        putchar(io, 33);
    } else {
        putchar(io, 46);
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
