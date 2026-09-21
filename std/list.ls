module std.list;

import std.option;

// `std.list` — a singly-linked list, and the **one collection here that
// holds resources**.
//
// `docs/collections.md` is why, and the reason is the shape rather than
// the generics. A list is a chain of boxes, and `unbox` hands back what
// a box held — so taking a list apart *produces* its elements, one at a
// time, and the walk that reads the list is the walk that frees it.
// `docs/heap.md` §3.1 makes a forgotten node a compile error, and that
// is as true of `List[Ticket]` as it was of the monomorphic list in
// `tests/accept/linked_list.ls`.
//
// The array-shaped collection cannot do this, which is `std.vec`'s
// bound and the doc's §2: freeing a run of elements is one `free` that
// **runs nothing**, so an obligation inside it would be dropped rather
// than discharged — and there is no destructor to hang the discharge on,
// by design.
//
// One allocation per element is the price. That is a real cost and the
// doc does not pretend otherwise; what it buys is the only container a
// program can put a resource in.

pub enum List[T] {
    Empty,
    Cons(T, Box[List[T]]),
}

// Add one to the front. Generic over the mode, because moving a `T`
// into the node is all this does — and moving is the thing a generic
// function *can* do to a resource (`docs/collections.md` §4).
pub fn push[T, &h](heap: &!h Heap, rest: List[T], value: T) -> [heap] List[T] {
    return List::Cons(value, box(heap, rest));
}

// Take one off the front: the head and the rest, or nothing.
//
// This is the operation the whole module is built around, and its
// signature is the rule. It consumes the list, frees exactly the one
// node it took apart, and hands the element **back** — so it discharges
// nothing and the caller owes the `T` afterwards. A `pop` that dropped
// the element instead would be `[T: val]`, and then there would be no
// way to get a resource out of a list at all.
pub fn pop[T, &h](heap: &!h Heap, list: List[T]) -> [heap] option.Option[(T, List[T])] {
    match list {
        List::Empty => { return option.Option::None; }
        List::Cons(value, rest) => {
            let tail = unbox(heap, rest);
            return option.Option::Some((value, tail));
        }
    }
}

// How many. By reference, so it counts a list of resources without
// owning one.
pub fn length[T, &l](list: &l List[T]) -> [] int {
    match list {
        List::Empty => { return 0; }
        List::Cons(_, rest) => { return 1 + length(contents(rest)); }
    }
}

// End the list, answering how many elements it had.
//
// `[T: val]` and there is no way around it: ending a list means ending
// every element, and a generic function does not know how to end a `T`.
// Over a resource element the caller writes this loop itself — which is
// not a gap in the library so much as the language saying that only the
// owner of a `Ticket` knows what ending one means.
//
// Recursive rather than a `while`, and that is forced: a loop's
// residual is a whole `List[T]` the checker cannot see is empty, so the
// obligation survives the loop. The recursion ends on `Empty`, which a
// `match` consumes outright.
pub fn drop[T: val, &h](heap: &!h Heap, list: List[T]) -> [heap] int {
    match list {
        List::Empty => { return 0; }
        List::Cons(_, rest) => {
            let tail = unbox(heap, rest);
            return 1 + drop(heap, tail);
        }
    }
}
