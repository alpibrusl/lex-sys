module std.buffer;

// `std.buffer` — a growable byte buffer, and the library's one data
// structure.
//
// `docs/boxed-slices.md` §4: there is no `grow`, `push` or `realloc` in
// the language, and that is deliberate. Growing is *allocate a bigger
// one, copy, end the old one* — every part of which is already
// expressible — so writing it here rather than as a builtin keeps the
// **policy** with a program that can read it. Putting it in `std`
// changed exactly one thing: the doubling policy is now written once
// instead of once per program. It is still policy, still in a library,
// and still not in the compiler.
//
// `docs/standard-library.md` §4 says why this is the *only* collection
// here: every other one wants generics over a mode, and §12 of
// `linearity-and-effects.md` does not have them. A byte buffer needs no
// mode polymorphism, which is why it is the one that exists.
//
// A `Buffer` owns its allocation, so it is `res`: the checker will not
// let a program forget one. `drop` is the only thing that ends one.

pub res struct Buffer {
    // What is allocated. `used <= len(contents(held))` always.
    held: Box[[byte]],
    used: int,
}

pub fn empty[&h](heap: &!h Heap, capacity: int) -> [heap] Buffer {
    return Buffer { held: box_slice(heap, capacity, byte_of(0)), used: 0 };
}

// End the buffer, returning how many bytes were in it.
pub fn drop[&h](heap: &!h Heap, b: Buffer) -> [heap] int {
    let Buffer { held, used } = b;
    unbox_slice(heap, held);
    return used;
}

// How many bytes are in it.
//
// Named `size` rather than `len` because `len` is a builtin and a
// program may not redeclare one — which is the right refusal, and worth
// having hit here rather than in someone else's code.
pub fn size[&b](b: &b Buffer) -> [] int {
    return b.used;
}

// Room for `more` bytes, doubling until there is.
//
// This is the whole of "growing": a bigger box, a copy, and the old one
// ended. `held` is a `res` value the destructuring produced, so a
// version that forgot the `unbox_slice` would not build — which is what
// makes a leak impossible rather than merely unlikely (`heap.md` §3.1).
pub fn reserve[&h](heap: &!h Heap, b: Buffer, more: int) -> [heap] Buffer {
    let Buffer { held, used } = b;

    var capacity = 0;
    borrow held as &r in {
        capacity = len(contents(r));
    }
    if used + more <= capacity {
        return Buffer { held: held, used: used };
    }

    var wanted = capacity;
    if wanted < 1 {
        wanted = 1;
    }
    while wanted < used + more {
        wanted = wanted * 2;
    }

    let bigger = box_slice(heap, wanted, byte_of(0));
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
    return Buffer { held: bigger, used: used };
}

pub fn push[&h](heap: &!h Heap, b: Buffer, value: byte) -> [heap] Buffer {
    let room = reserve(heap, b, 1);
    let Buffer { held, used } = room;
    borrow mut held as &!w in {
        let s = contents(w);
        s[used] = value;
    }
    return Buffer { held: held, used: used + 1 };
}

pub fn append[&h, &r](heap: &!h Heap, b: Buffer, text: &r [byte]) -> [heap] Buffer {
    var out = reserve(heap, b, len(text));
    var i = 0;
    while i < len(text) {
        out = push(heap, out, text[i]);
        i = i + 1;
    }
    return out;
}

// Decimal, most significant digit first.
pub fn push_nat[&h](heap: &!h Heap, b: Buffer, n: int) -> [heap] Buffer {
    var out = b;
    if n >= 10 {
        out = push_nat(heap, out, n / 10);
    }
    return push(heap, out, byte_of(48 + n % 10));
}

// Print what is in the buffer.
//
// By reference, which took two slices to become possible. A `Buffer`
// owns its box, and a `res` field could not be reached through a
// reference at all — so this used to take the buffer by value and hand
// it back, and every caller had to thread the result. `b.held` is a
// `&b Box[[byte]]` now (`docs/reading-references.md` §2.0), and the
// buffer is not disturbed by being printed.
pub fn write[&i, &b](io: &!i Io, b: &b Buffer) -> [io_write] int {
    let whole = contents(b.held);
    var i = 0;
    while i < b.used {
        putchar(io, int_of(whole[i]));
        i = i + 1;
    }
    return b.used;
}
