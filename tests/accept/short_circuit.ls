// `&&` and `||` do not evaluate their right operand when the left already
// decides the answer. `noisy` prints 'X', so the test is that 'X' never
// appears — a fixture that would still pass if the operators were strict is
// not a test of short-circuiting.
//~ STDOUT ab
//~ EXIT 0

fn noisy[&i](io: &!i Io) -> [io_write] bool {
    putchar(io, 88);                     // 'X'
    return true;
}

fn run[&i](io: &!i Io) -> [io_write] int {
    if false && noisy(io) {
        putchar(io, 63);                 // '?'
    }
    if true || noisy(io) {
        putchar(io, 97);                 // 'a'
    }
    putchar(io, 98);                     // 'b'
    putchar(io, 10);
    return 0;
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
