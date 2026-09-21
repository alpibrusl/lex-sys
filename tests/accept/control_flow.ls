// Recursion, `while`, `if`/`else if`/`else`, mutable locals.
//~ STDOUT 01123A
//~ EXIT 0

fn fib(n: int) -> [] int {
    if n < 2 {
        return n;
    } else {
        return fib(n - 1) + fib(n - 2);
    }
}

fn run[&i](io: &!i Io) -> [io_write] int {
    var i = 0;
    while i < 5 {
        putchar(io, 48 + fib(i));
        i = i + 1;
    }
    if i == 5 {
        putchar(io, 65);
    } else if i == 4 {
        putchar(io, 66);
    } else {
        putchar(io, 67);
    }
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
