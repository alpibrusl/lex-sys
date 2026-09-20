// `docs/reading-references.md` §2 and §4: the traversal that was not
// expressible until now.
//
// `examples/tree.ls` had to compute everything in the pass that frees,
// because `match` required ownership and taking a list apart to look at it
// *was* taking it apart. A structure you can only read by consuming is a
// structure you can read once, and "traverse it twice" is not exotic --
// it is `len` then `sum`.
//
// Here the list is read three times and freed once. Nothing in the reading
// functions is heap machinery: they take `&l List`, match it, and the
// bindings come back as references into it.
//~ STDOUT 10 3 10
//~ STDOUT freed 10
//~ EXIT 0

enum List {
    Empty,
    Cons(int, Box[List]),
}

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

fn push[&h](heap: &!h Heap, rest: List, value: int) -> [heap] List {
    return List::Cons(value, box(heap, rest));
}

// Read-only, and its row says so: `[]`. It touches no capability at all,
// which is the strongest possible statement that it does not free
// anything.
//
// `value` is `&l int`, so `*value` reads it. `rest` is `&l Box[List]`, so
// `contents` follows the box to `&l List` and the recursion borrows for
// exactly as long as this frame does.
fn total[&l](list: &l List) -> [] int {
    match list {
        List::Empty => { return 0; }
        List::Cons(value, rest) => { return *value + total(contents(rest)); }
    }
}

fn length[&l](list: &l List) -> [] int {
    match list {
        List::Empty => { return 0; }
        // `_` through a reference discards nothing: the match never owned
        // the value, so there is nothing here to drop.
        List::Cons(_, rest) => { return 1 + length(contents(rest)); }
    }
}

// The consuming traversal, unchanged and still the only way to free the
// list: it takes the list rather than a reference to it.
fn drain[&h](heap: &!h Heap, list: List) -> [heap] int {
    match list {
        List::Empty => { return 0; }
        List::Cons(value, rest) => {
            let tail = unbox(heap, rest);
            return value + drain(heap, tail);
        }
    }
}

fn run[&h, &i](heap: &!h Heap, io: &!i Io) -> [heap, io] int {
    var list = List::Empty;
    list = push(heap, list, 5);
    list = push(heap, list, 1);
    list = push(heap, list, 4);

    borrow list as &l in {
        print_nat(io, total(l));
        putchar(io, 32);
        print_nat(io, length(l));
        putchar(io, 32);
        // Again, because it can be: the first read did not spend it.
        print_nat(io, total(l));
    }
    putchar(io, 10);

    write_all(io, "freed ");
    let freed = drain(heap, list);
    print_nat(io, freed);
    putchar(io, 10);
    return freed;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
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
    return status - 10;
}
