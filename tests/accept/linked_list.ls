// `docs/heap.md` §4: the declaration that did not compile yesterday.
//
//     enum List { Empty, Cons(int, List) }
//         error: type `List` contains itself, so it has no finite size
//
// One `Box` on the path back to itself and it has a size, because a box is
// a pointer however large what it points at is. Every linked structure in
// computing is that declaration plus this indirection.
//
// What is worth reading is the traversal. In a linear language the walk
// that reads the list is the walk that *frees* it -- `rest` is a `res`
// value the match handed over, `unbox` is the only thing that ends one, and
// a version of this function that forgot a node would not compile. The
// traversal and the proof that it freed everything are the same code.
//~ STDOUT 3 1 4 1 5
//~ STDOUT 14
//~ EXIT 0

enum List {
    Empty,
    Cons(int, Box[List]),
}

fn print_nat[&i](io: &!i Io, n: int) -> [io_write] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, '0' + n % 10);
}

// Build backwards: each `box` allocates one node and takes ownership of the
// tail. What comes back is one `res` value that owns every node in it, so
// the caller owes exactly one traversal that ends it.
fn push[&h](heap: &!h Heap, rest: List, value: int) -> [heap] List {
    return List::Cons(value, box(heap, rest));
}

// One pass: print each value and free the node that held it.
//
// `rest` is the box the match produced. `unbox` frees that node and yields
// the tail, and the recursion does the same to it -- exactly one `free` per
// node, guaranteed by the checker rather than by a convention or a test.
fn drain[&h, &i](heap: &!h Heap, io: &!i Io, list: List, first: bool) -> [heap, io_write] int {
    match list {
        List::Empty => { return 0; }
        List::Cons(value, rest) => {
            if first == false {
                putchar(io, 32);
            }
            print_nat(io, value);
            let tail = unbox(heap, rest);
            return value + drain(heap, io, tail, false);
        }
    }
}

fn run[&h, &i](heap: &!h Heap, io: &!i Io) -> [heap, io_write] int {
    var list = List::Empty;
    list = push(heap, list, 5);
    list = push(heap, list, 1);
    list = push(heap, list, 4);
    list = push(heap, list, 1);
    list = push(heap, list, 3);

    // The only path that ends the list, and it frees every node on the way.
    let total = drain(heap, io, list, true);
    putchar(io, 10);
    print_nat(io, total);
    putchar(io, 10);
    return total;
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
    return status - 14;
}
