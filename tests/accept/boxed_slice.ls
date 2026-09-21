// `docs/boxed-slices.md` §3: allocate, write through it, read it back,
// end it.
//
// What is worth reading is `build`: it makes a run of values and
// **returns it**. An arena slice cannot do that -- §6's escape check
// exists to stop it, because the memory goes away with the block. A boxed
// slice is the shape whose lifetime the program decides, which is what
// every collection needs and what nothing here had until now.
//~ STDOUT ABCDE
//~ STDOUT 5 bytes, freed 5
//~ EXIT 0

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

// Built here, used by the caller: the box outlives this frame.
fn build[&h](heap: &!h Heap, count: int) -> [heap] Box[[byte]] {
    let b = box_slice(heap, count, byte_of(65));
    borrow mut b as &!w in {
        // `contents` is the same `contents` an ordinary box has, and it is
        // mode-preserving: a unique borrow of the box gives a unique slice,
        // so this may write. Two leaves come back rather than one, because
        // a boxed slice carries its length (§2).
        let s = contents(w);
        var i = 0;
        while i < len(s) {
            s[i] = byte_of(65 + i);
            i = i + 1;
        }
    }
    return b;
}

fn run[&h, &i](heap: &!h Heap, io: &!i Io) -> [heap, io] int {
    let buffer = build(heap, 5);

    var size = 0;
    borrow buffer as &r in {
        // Shared this time, so `contents` gives a shared slice: readable,
        // and nothing may write through it.
        write_all(io, contents(r));
        putchar(io, 10);
        size = len(contents(r));
    }

    // The only thing that ends a boxed slice, and it answers how many
    // elements it freed.
    let freed = unbox_slice(heap, buffer);

    print_nat(io, size);
    write_all(io, " bytes, freed ");
    print_nat(io, freed);
    putchar(io, 10);
    return freed;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
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
    return status - 5;
}
