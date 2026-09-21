// §7: a row that is exactly what the body performs, all the way up.
//
// Three things this shows and the rejects cannot:
//
//   * a pure helper stays pure. `double` touches nothing, so its row is `[]`,
//     and that is a fact a reader can act on rather than an absence.
//   * a row is transitive. `banner` performs `io_write` because `emit` does, and
//     `main` because `banner` does.
//   * a row is a *set*. `[io_write, io_write]` is `[io_write]`, and `[io_write]` written twice in the
//     body costs nothing extra -- union, not count.
//~ STDOUT AB6
//~ EXIT 0

// Nothing here reaches the console, so the row is empty and says so.
fn double(n: int) -> [] int {
    return n * 2;
}

// The grounding: `putchar` is the builtin that performs `io_write`, and every `io`
// in every row above this one traces back to it.
fn emit[&i](io: &!i Io, c: int) -> [io_write] int {
    return putchar(io, c);
}

// Performs `io_write` twice; the row is still `[io_write]`, because a row is a set.
fn banner[&i](io: &!i Io) -> [io_write] int {
    emit(io, 65);
    return emit(io, 66);
}

fn run[&i](io: &!i Io) -> [io_write] int {
    banner(io);
    // A pure call inside an effectful function adds nothing to the row.
    let last = emit(io, 48 + double(3));
    emit(io, 10);
    return last - 54;
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
