// hello.ls — the smoke program (#3), and the narrowest program the language
// can express.
//
// For three milestones this file packed its greeting into two 64-bit words
// and unpacked it a byte at a time, because there were no strings. M3's
// `docs/strings.md` is what removed the workaround, and this file is the
// clearest signal it landed: the greeting is now a greeting.
//
// What is left is not boilerplate. `main` takes the `World` the runtime
// hands it, splits it, and threads the console capability down to the one
// function that writes — delete the `io` parameter from `write_all` and the
// body stops compiling. That is the language's whole argument in ten lines.
//~ STDOUT Hello, world!
//~ EXIT 0

// A string is `&r [byte]`: an ordinary slice, so an ordinary reference.
// `len` reads the length that travels beside the pointer, and every index
// is bounds-checked.
fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io_write] int {
    var n = 0;
    while n < len(s) {
        putchar(io, int_of(s[n]));
        n = n + 1;
    }
    return len(s);
}

fn main(world: World) -> [] int {
    // §8.2: the runtime hands over exactly one `World`, and `split` consumes
    // it. There is no other way to obtain a capability.
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // This program touches no files, so that authority ends here.
    release(heap);
    release(fs);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);

    var written = 0;
    // Threaded by borrow, not by move: a callee should not consume its
    // caller's authority.
    borrow mut io as &!i in {
        written = write_all(i, "Hello, world!\n");
    }

    // Authority is a resource, so it is destroyed exactly once. A program
    // that forgets this does not compile.
    release(io);
    if written == 14 {
        return 0;
    }
    return 1;
}
