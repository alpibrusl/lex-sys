// `docs/heap.md` §3: box a value, read it through a borrow, write through
// a unique one, unbox it.
//
// Nothing here is heap machinery beyond the three calls. The borrow is §5's
// borrow, the field access is M1's field access, and `contents` is
// mode- and region-preserving, so a shared borrow of the box gives a shared
// borrow of what it holds and a unique one gives a unique one. That is the
// claim the design keeps making: a new kind of memory needed no new kind of
// anything else.
//~ STDOUT 7
//~ STDOUT 10 4
//~ EXIT 0

fn print_nat[&i](io: &!i Io, n: int) -> [io] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, 48 + n % 10);
}

struct Point { x: int, y: int }

fn run[&h, &i](heap: &!h Heap, io: &!i Io) -> [heap, io] int {
    // One `malloc`. The box is a `res` value from here on, so the program
    // owes exactly one `unbox` on every path out of this function.
    let b = box(heap, Point { x: 3, y: 4 });

    var sum = 0;
    borrow b as &r in {
        // `contents(r)` is `&r Point` -- a shared reference, for exactly as
        // long as `r` lasts.
        sum = contents(r).x + contents(r).y;
    }
    print_nat(io, sum);
    putchar(io, 10);

    borrow mut b as &!w in {
        // Unique, so this may write. The box is locked for the whole block.
        contents(w).x = 10;
    }

    // One `free`, and the value comes back out. A `res` inside a box would
    // come back out here too, obligation intact.
    let p = unbox(heap, b);
    print_nat(io, p.x);
    putchar(io, 32);
    print_nat(io, p.y);
    putchar(io, 10);
    return sum;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap } = split(world);
    release(ffi);
    release(fs);

    var status = 0;
    borrow mut heap as &!h in {
        borrow mut io as &!i in {
            status = run(h, i);
        }
    }
    release(heap);
    release(io);
    return status - 7;
}
