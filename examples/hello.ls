// hello.ls — the smoke program (#3), and the narrowest program the language
// can express.
//
// It is still written the way M0 forced: there are no strings and no arrays
// even now, so the greeting travels as two packed 64-bit words, seven bytes
// each, unpacked a byte at a time. M3's slices are what change that.
//
// Deliberately left in its original form. The language has grown a type
// system, structs, enums and generics since — see `tour.ls` for those — and
// this file is worth keeping as the thing CI has built and run on both targets
// since the first milestone.
//
//~ STDOUT Hello, world!
//~ EXIT 0

// Write the low seven bytes of `word`, least significant first.
fn put_word[&i](io: &!i Io, word: int) -> [io] int {
    var rest = word;
    var written = 0;
    while rest > 0 {
        putchar(io, rest % 256);
        rest = rest / 256;
        written = written + 1;
    }
    return written;
}

// "Hello, " and "world!\n", little-endian in base 256.
fn greeting_head() -> [] int {
    return 9056056326776136;
}

fn greeting_tail() -> [] int {
    return 2851464966991735;
}

fn run[&i](io: &!i Io) -> [io] int {
    let written = put_word(io, greeting_head()) + put_word(io, greeting_tail());
    if written == 14 {
        return 0;
    } else {
        // Unreachable unless codegen is wrong, and then the exit status says so.
        return 1;
    }
}

fn main(world: World) -> [] int {
    // §8.2: the runtime hands over exactly one `World`, and `split` consumes
    // it. There is no other way to obtain a capability.
    let Split { io, ffi } = split(world);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    var status = 0;
    // Threaded by borrow, not by move: a callee should not consume its
    // caller's authority.
    borrow mut io as &!i in {
        status = run(i);
    }
    // Authority is a resource, so it is destroyed exactly once. A program
    // that forgets this does not compile.
    release(io);
    return status;
}
