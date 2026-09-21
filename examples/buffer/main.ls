// `buffer` — building a string whose length nothing knew in advance.
//
// This is what boxed slices are for. Before them a run of values lived in
// an arena, bounded by its block, or nowhere; a buffer that grows as it is
// written could not exist, and neither could any collection.
//
// `buffer.ls` is the library and holds all of the interesting code — in
// particular `reserve`, which is the entire implementation of "growing": a
// bigger box, a copy, and the old one ended. The language has no `grow`,
// `push` or `realloc` on purpose (`docs/boxed-slices.md` §4), so the
// doubling policy below belongs to this program rather than to lex-sys.
//
// Run it:
//
//     lex-sys build examples/buffer/main.ls examples/buffer/buffer.ls -o buffer
//     ./buffer
//
// Watch the capacity. The buffer starts at one byte and doubles, and
// `push_all` reserves for a whole run at once rather than a byte at a
// time -- so building these 31 bytes allocates three times in total
// (1, then 16, then 32) and copies what it had on each grow. Under
// valgrind that is exactly three allocations and three frees, plus the
// one stdio makes.
//
// That cost is countable because the policy is written down, in
// `reserve`, in this repository. A `realloc` builtin would have hidden
// which of two very different things happened.

fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io_write] int {
    var n = 0;
    while n < len(s) {
        putchar(io, int_of(s[n]));
        n = n + 1;
    }
    return len(s);
}

fn run[&h, &i](heap: &!h Heap, io: &!i Io) -> [heap, io_write] int {
    // One byte of capacity, on purpose: everything after this is growth.
    var text = new_buffer(heap, 1);

    text = push_all(heap, text, "counting:");
    var n = 1;
    while n <= 8 {
        text = push_byte(heap, text, byte_of(32));
        text = push_nat(heap, text, n * n);
        n = n + 1;
    }
    text = push_byte(heap, text, byte_of(10));

    // Read it back before ending it. `write_buffer` takes the buffer by
    // value and hands it back, which is not a style choice: a `Buffer`
    // owns its box, and a `res` field cannot be read through a reference,
    // so reaching the box means owning the buffer.
    text = write_buffer(io, text);

    // The only path that ends the buffer, and it ends the box with it.
    return release_buffer(heap, text);
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
    return status - 31;
}
