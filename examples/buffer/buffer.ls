// `buffer/buffer.ls` — a growable byte buffer, as a library.
//
// `docs/boxed-slices.md` §4: there is no `grow`, `push` or `realloc` in
// the language, and that is deliberate. Growing is *allocate a bigger one,
// copy, end the old one* — every part of which is already expressible — so
// writing it here rather than as a builtin keeps the **policy** with the
// program that chose it. This file doubles; a different buffer could add a
// fixed amount, and the language would not have an opinion.
//
// A `Buffer` owns its box, so it is `res`: the checker will not let a
// program forget one. Every function below either hands it back or ends
// it, and `release_buffer` is the only thing that ends one.

res struct Buffer {
    // What is allocated. `used <= len(contents(box))` always.
    held: Box[[byte]],
    used: int,
}

fn new_buffer[&h](heap: &!h Heap, capacity: int) -> [heap] Buffer {
    return Buffer { held: box_slice(heap, capacity, byte_of(0)), used: 0 };
}

fn release_buffer[&h](heap: &!h Heap, b: Buffer) -> [heap] int {
    let Buffer { held, used } = b;
    unbox_slice(heap, held);
    return used;
}

// Room for `more` bytes, doubling until there is.
//
// This is the whole of "growing": a bigger box, a copy, and the old one
// ended. `old` is a `res` value the destructuring produced, so the
// compiler will not let this function drop it -- a version that forgot the
// `unbox_slice` would not build, which is the property that makes a leak
// impossible rather than merely unlikely (`heap.md` §3.1).
fn reserve[&h](heap: &!h Heap, b: Buffer, more: int) -> [heap] Buffer {
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

fn push_byte[&h](heap: &!h Heap, b: Buffer, value: byte) -> [heap] Buffer {
    let room = reserve(heap, b, 1);
    let Buffer { held, used } = room;
    borrow mut held as &!w in {
        let s = contents(w);
        s[used] = value;
    }
    return Buffer { held: held, used: used + 1 };
}

fn push_all[&h, &r](heap: &!h Heap, b: Buffer, text: &r [byte]) -> [heap] Buffer {
    var out = reserve(heap, b, len(text));
    var i = 0;
    while i < len(text) {
        out = push_byte(heap, out, text[i]);
        i = i + 1;
    }
    return out;
}

// Print the used bytes, and hand the buffer back.
//
// It takes the `Buffer` **by value** rather than by reference, and that is
// forced rather than stylistic: a `Buffer` owns its box, and a `res` field
// cannot be read through a reference
// (`docs/reading-references.md` §2 -- nothing moves out of a reference).
// So the way to reach the box is to take the whole buffer apart, which
// means owning it, which means handing it back.
fn write_buffer[&i](io: &!i Io, b: Buffer) -> [io_write] Buffer {
    let Buffer { held, used } = b;
    borrow held as &r in {
        let whole = contents(r);
        var i = 0;
        while i < used {
            putchar(io, int_of(whole[i]));
            i = i + 1;
        }
    }
    return Buffer { held: held, used: used };
}

// Decimal, most significant digit first. Recursive because the digits come
// out backwards otherwise.
fn push_nat[&h](heap: &!h Heap, b: Buffer, n: int) -> [heap] Buffer {
    var out = b;
    if n >= 10 {
        out = push_nat(heap, out, n / 10);
    }
    return push_byte(heap, out, byte_of('0' + n % 10));
}
