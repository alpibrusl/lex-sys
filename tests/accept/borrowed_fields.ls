// `docs/reading-references.md` §2.0: a `res` field through a reference
// is a **borrow**.
//
// The rule it replaces was a refusal, and the refusal was right about
// soundness and wrong about scope. Copying a `res` field out of a
// reference made a second owner and a double free; refusing to read one
// at all meant a struct with a `res` field could not be read through a
// reference *even to look at it*, while the equivalent enum could.
//
// So what comes back is a reference, carrying the base's mode and its
// region. The double free stays unexpressible without any rule about
// reading: `unbox` wants a `Box`, and `&r Box` is not one.
//
// Printing the buffer **twice** is the whole point. Under the old rule
// `write` took the buffer by value and handed it back, so reading it
// spent it and every caller threaded the result through.
//~ STDOUT counting 1 4 9
//~ STDOUT counting 1 4 9
//~ STDOUT 709 3
//~ EXIT 0

import std.buffer;
import std.vec;
import std.io as console;

res struct Holder {
    held: Box[int],
}

// Look at a `res` field without owning, spending or disturbing it. The
// binding is a `&r Box[int]`, and `contents` follows it exactly as it
// follows any other borrowed box.
fn peek[&r](holder: &r Holder) -> [] int {
    let box_ref = holder.held;
    return *contents(box_ref);
}

// A tuple is an anonymous struct, so `.0` obeys `.x`'s rule.
fn peek_pair[&p](pair: &p (Box[int], int)) -> [] int {
    let box_ref = pair.0;
    return *contents(box_ref) + pair.1;
}

// A unique base gives a unique field reference: the fields of one value
// are disjoint, which is §2.2's argument for payloads in the place
// fields want it.
fn bump[&r](holder: &!r Holder) -> [] int {
    let box_ref = holder.held;
    return *contents(box_ref) + 1;
}

fn run[&h, &i](heap: &!h Heap, io: &!i Io) -> [heap, io_write] int {
    var total = 0;

    // A boxed field, read three ways and freed exactly once.
    var holder = Holder { held: box(heap, 41) };
    borrow holder as &r in {
        total = total + peek(r);
    }
    borrow mut holder as &!r in {
        total = total + bump(r);
    }
    let Holder { held } = holder;
    total = total + unbox(heap, held);

    let pair = (box(heap, 7), 1);
    borrow pair as &p in {
        total = total + peek_pair(p);
    }
    let (held, tag) = pair;
    total = total + unbox(heap, held) + tag;

    // `std.buffer`, printed twice through a reference. The buffer is not
    // disturbed by being read, which is what a reference is for.
    var b = buffer.empty(heap, 4);
    b = buffer.append(heap, b, "counting");
    var n = 1;
    while n < 4 {
        b = buffer.push(heap, b, byte_of(32));
        b = buffer.push_nat(heap, b, n * n);
        n = n + 1;
    }
    var written = 0;
    borrow b as &r in {
        written = buffer.write(io, r);
        console.newline(io);
        written = written + buffer.write(io, r);
        console.newline(io);
    }
    total = total + buffer.drop(heap, b) + written;

    // And `std.vec`'s getter, which is a getter again.
    var v = vec.empty(heap, 2, 0);
    v = vec.push(heap, v, 7);
    v = vec.push(heap, v, 0);
    v = vec.push(heap, v, 9);
    var first = 0;
    var third = 0;
    var count = 0;
    borrow v as &r in {
        first = vec.get(r, 0);
        third = vec.get(r, 2);
        count = vec.size(r);
    }
    console.print_int(io, first * 100 + third);
    putchar(io, 32);
    console.print_int(io, count);
    console.newline(io);
    total = total + vec.drop(heap, v);

    return total;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    // This program touches no files, so that authority ends here.
    release(fs);

    var status = 0;
    borrow mut heap as &!h in {
        borrow mut io as &!i in {
            status = run(h, i);
        }
    }
    release(heap);
    release(io);
    return status - 185;
}
