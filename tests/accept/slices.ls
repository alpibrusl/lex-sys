// M3's first half: slices, which are what strings will be made of.
//
// A slice is an ordinary reference. `[T]` is a *referent* — a run of `T`s
// whose length is a runtime value, unsized like a struct is sized — and
// `&!a [T]` points at one. That is the whole design: every rule §5 gave
// references applies to a slice without a second mechanism, so a slice
// carries a region, cannot escape it, coerces from unique to shared, and
// is `val` because every reference is.
//
// The length travels *in* the slice, which is why `len` reads a value
// rather than computing one and why indexing can check it.
//~ STDOUT 0 1 4 9 16 -> 30
//~ STDOUT evens: 2 4 6
//~ EXIT 0

fn space[&i](io: &!i Io) -> [io] int {
    return putchar(io, 32);
}

fn print_nat[&i](io: &!i Io, n: int) -> [io] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, 48 + n % 10);
}

// Takes a *shared* slice, and is called below on what `alloc_slice` handed
// back as unique: `&!r [T]` is `&r [T]` plus permission to write.
fn total[&r](xs: &r [int]) -> [] int {
    var sum = 0;
    var i = 0;
    while i < len(xs) {
        sum = sum + xs[i];
        i = i + 1;
    }
    return sum;
}

fn show[&r, &i](io: &!i Io, xs: &r [int]) -> [io] int {
    var i = 0;
    while i < len(xs) {
        if i > 0 {
            space(io);
        }
        print_nat(io, xs[i]);
        i = i + 1;
    }
    return len(xs);
}

// Writing through a unique slice. `xs[i] = v` is a place, bounds-checked
// exactly as a read is -- `docs/defined-behaviour.md` §1: an index outside
// the slice traps, because the alternative is reading past the end of an
// allocation, and this language has no undefined behaviour to do that in.
fn squares[&r](xs: &!r [int]) -> [] int {
    var i = 0;
    while i < len(xs) {
        xs[i] = i * i;
        i = i + 1;
    }
    return len(xs);
}

fn run[&i](io: &!i Io) -> [io] int {
    var answer = 0;
    region a {
        // Five elements, all zero to begin with. The length is a runtime
        // value, which is what makes this a slice and not an array: an
        // array's length lives in its type.
        let xs = alloc_slice[a](5, 0);
        squares(xs);
        show(io, xs);
        putchar(io, 32); putchar(io, 45); putchar(io, 62); space(io);   // " -> "
        answer = total(xs);
        print_nat(io, answer);
        putchar(io, 10);

        // A second slice in the same arena, filled from a computation. Both
        // go when the block closes, in one call.
        let evens = alloc_slice[a](3, 0);
        var i = 0;
        while i < len(evens) {
            evens[i] = 2 * i + 2;
            i = i + 1;
        }
        putchar(io, 101); putchar(io, 118); putchar(io, 101); putchar(io, 110);
        putchar(io, 115); putchar(io, 58); space(io);                   // "evens: "
        show(io, evens);
        putchar(io, 10);
    }
    return answer;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs } = split(world);
    // This program touches no files, so that authority ends here.
    release(fs);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    var status = 0;
    borrow mut io as &!i in {
        status = run(i);
    }
    release(io);
    return status - 30;
}
