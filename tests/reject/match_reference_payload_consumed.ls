//~ ERROR expected

// §2: "a reference gives references", and nothing moves out of one.
//
// `rest` here is a `&l Box[List]` -- a *borrow* of the box, not the box.
// So the operation that would end it does not typecheck against it, and
// linearity never has to intervene: the list is as owned after this match
// as it was before, which is what makes reading it non-destructive.
//
// The consuming traversal is still written, and still the only way to free
// the list. It just takes the list rather than a reference to it.

enum List {
    Empty,
    Cons(int, Box[List]),
}

fn steal[&h, &l](heap: &!h Heap, list: &l List) -> [heap] int {
    match list {
        List::Empty => { return 0; }
        List::Cons(value, rest) => {
            // `unbox` ends a `Box`. This is a reference to one.
            let tail = unbox(heap, rest);
            return 1;
        }
    }
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    return 0;
}
