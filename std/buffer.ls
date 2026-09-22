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

// The bytes the buffer holds — exactly `used` of them, not the whole
// allocation.
//
// This is the function `docs/slicing.md` §6 was written for. A buffer
// holds more than it uses, so handing its contents to anything taking a
// `&r [byte]` means saying *which* of them, and until `[a..b]` existed
// there was no way to say it. That is why the library had a `write` of
// its own instead of formatting into a buffer and emitting it, and why
// there is no `Writer` type: the buffer is the abstraction, and this is
// how it crosses to one.
pub fn bytes[&b](b: &b Buffer) -> [] &b [byte] {
    return contents(b.held)[0..b.used];
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
    let whole = bytes(b);
    var i = 0;
    while i < len(whole) {
        putchar(io, int_of(whole[i]));
        i = i + 1;
    }
    return len(whole);
}

// The unused tail, for something that writes bytes itself to fill.
//
// `fs_read` takes a `&!r [byte]` and writes into it
// (`docs/filesystem.md` §3), so a program reading a file into a buffer
// needs the room *before* it knows how much will be used. Until a port
// wanted exactly that there was no way to ask: `push` and `append` both
// take the bytes as an argument, which is the wrong direction when the
// filesystem is the one producing them (`docs/porting.md` §9).
//
// Unique, because the caller writes through it. What it hands back is a
// reference into this buffer's allocation, so it lives as long as the
// borrow and no longer.
pub fn room[&b](b: &!b Buffer) -> [] &!b [byte] {
    let s = contents(b.held);
    return s[b.used..len(s)];
}

// Empty it, keeping the allocation.
//
// The one function `examples/cut/` asked for, and it asked by not being
// able to do without it: a program reading a line at a time has to
// reuse a buffer, and `drop` plus `empty` per line is an allocation per
// line rather than one for the program. There was no other way to move
// `used` back — `filled` only goes forward — so this is a gap rather
// than a convenience (`docs/line-reading.md` §4).
//
// The allocation stays, which is the point: the next line writes into
// the room the last one already grew.
pub fn clear[&b](b: &!b Buffer) -> [] int {
    b.used = 0;
    return 0;
}

// Commit `n` bytes that `room` was just filled with.
//
// Separate from `room` because the two answer different questions and
// only the caller knows the second: `fs_read` says how much it wrote,
// and a buffer cannot see a write it did not make. Traps if `n` would
// take `used` past the allocation, which is the same bounds check every
// other operation here gets, arriving one step later.
pub fn filled[&b](b: &!b Buffer, n: int) -> [] int {
    let s = contents(b.held);
    // `s[b.used..b.used + n]` is the range that was written; evaluating
    // it is the check, and an out-of-range commit traps here rather than
    // corrupting `used` for every later reader.
    let written = s[b.used..b.used + n];
    b.used = b.used + len(written);
    return b.used;
}
