module std.vec;

// `std.vec` — a growable run of values, one allocation for all of them.
//
// This is `std.buffer` with the element type lifted out, and the lift
// does **not** go all the way: a `Vec[T]` holds its elements in a boxed
// slice, and a boxed slice holds `val` data only, for two independent
// reasons either of which alone would be enough (`docs/collections.md`
// §2):
//
//   * the fill is copied into every element, and a linear value cannot
//     be copied at all; and
//   * `unbox_slice` is one `free` that **runs nothing**, so an
//     obligation inside the run would be dropped rather than
//     discharged, which is the affine hole this language refuses.
//
// So the bound is `[T: val]`, written on the declaration, and that is
// the feature this module asked for. `res struct Vec[T: val]` says the
// vector *owns* an allocation while its elements are copyable — two
// modes in one declaration, which a `val`/`res` keyword alone cannot
// express, since the keyword speaks about the aggregate and this is
// about a parameter. A program reaching for `Vec[Ticket]` is refused
// where it wrote `Vec[Ticket]`, naming the bound, rather than somewhere
// inside this file it cannot change.
//
// `std.list` is the collection that holds resources, and the difference
// between the two is the **shape** rather than the generics.

pub res struct Vec[T: val] {
    // What is allocated. `used <= len(contents(held))` always.
    held: Box[[T]],
    used: int,
    // What the unused room holds, kept so `reserve` can seed a bigger
    // run without being handed one again.
    //
    // `T` is copyable, so a spare one is a copy rather than an
    // allocation: the bound is what makes keeping it free.
    fill: T,
}

// A vector with room for `capacity`, and `fill` is what the unused room
// holds. There is no uninitialised memory here to leave alone, so the
// caller supplies a value rather than the language inventing one.
pub fn empty[T: val, &h](heap: &!h Heap, capacity: int, fill: T) -> [heap] Vec[T] {
    return Vec { held: box_slice(heap, capacity, fill), used: 0, fill: fill };
}

// End it, answering how many elements it had.
//
// Unlike `std.list`'s, this `drop` needs no separate justification for
// its bound: the declaration already carries it, so there is no
// `Vec[T]` over a resource for it to fail on.
pub fn drop[T: val, &h](heap: &!h Heap, v: Vec[T]) -> [heap] int {
    let Vec { held, used, fill } = v;
    unbox_slice(heap, held);
    return used;
}

// How many elements are in it.
pub fn size[T: val, &v](v: &v Vec[T]) -> [] int {
    return v.used;
}

// Room for `more` elements, doubling until there is.
//
// The same allocate-copy-end that `std.buffer` writes, and it stays a
// library rather than becoming a builtin for the same reason: the
// doubling is **policy**, and policy belongs where a program can read it
// (`docs/boxed-slices.md` §4).
pub fn reserve[T: val, &h](heap: &!h Heap, v: Vec[T], more: int) -> [heap] Vec[T] {
    let Vec { held, used, fill } = v;

    var capacity = 0;
    borrow held as &r in {
        capacity = len(contents(r));
    }
    if used + more <= capacity {
        return Vec { held: held, used: used, fill: fill };
    }

    var wanted = capacity;
    if wanted < 1 {
        wanted = 1;
    }
    while wanted < used + more {
        wanted = wanted * 2;
    }

    let bigger = box_slice(heap, wanted, fill);
    borrow mut bigger as &!w in {
        borrow held as &r in {
            let to = contents(w);
            let from = contents(r);
            var i = 0;
            while i < used {
                to[i] = from[i];
                i = i + 1;
            }
        }
    }
    unbox_slice(heap, held);
    return Vec { held: bigger, used: used, fill: fill };
}

pub fn push[T: val, &h](heap: &!h Heap, v: Vec[T], value: T) -> [heap] Vec[T] {
    let room = reserve(heap, v, 1);
    let Vec { held, used, fill } = room;
    borrow mut held as &!w in {
        let s = contents(w);
        s[used] = value;
    }
    return Vec { held: held, used: used + 1, fill: fill };
}

// The `i`th element.
//
// By reference, and it reads like a getter should. It did not: a `res`
// field could not be reached through a reference at all, so this had to
// take the whole vector by value and hand it back in a tuple, and
// `collections.md` §5.1 recorded the cost. `reading-references.md` §2.0
// closed it — `v.held` is a `&v Box[[T]]`, a borrow of the box rather
// than a copy of it, exactly as `match` on a reference binds a `res`
// payload.
//
// The index is bounds-checked like every other one, so an out-of-range
// element traps rather than reading past the end.
pub fn get[T: val, &v](v: &v Vec[T], i: int) -> [] T {
    let s = contents(v.held);
    return s[i];
}

// Write one element.
//
// A vector could be read and appended to but never *written* until a
// program needed to sort one (`docs/porting.md` §9). The asymmetry was
// not a decision: `push` and `get` were what the first caller wanted,
// and nothing had asked for the third.
//
// The reference is unique because this writes through it, which is the
// same rule `fs_read`'s `into` follows (`docs/filesystem.md` §3). The
// index is bounds-checked like every other one.
pub fn set[T: val, &v](v: &!v Vec[T], i: int, value: T) -> [] int {
    let s = contents(v.held);
    s[i] = value;
    return i;
}

// Exchange two elements.
//
// Expressible with `get` and `set`, and here anyway because every sort
// writes it and because `T` is `val`: the reads are copies, so there is
// no moment where an element exists twice as an obligation.
pub fn swap[T: val, &v](v: &!v Vec[T], i: int, j: int) -> [] int {
    let s = contents(v.held);
    let held = s[i];
    s[i] = s[j];
    s[j] = held;
    return i;
}
