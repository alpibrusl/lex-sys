// `docs/compile-time.md`: what the compiler works out before the program
// runs.
//
// Every number below is computed at compile time and the binary holds
// only the answer. Nothing here is written differently from how it would
// be written without the pass — there is no `const` keyword and nothing
// to ask for (§3.1), which is the point: a program becomes cheaper
// without becoming stranger.
//~ STDOUT arithmetic 0
//~ STDOUT nested 120
//~ STDOUT recursion 55
//~ STDOUT bits 255
//~ STDOUT comparison 1
//~ STDOUT runtime 55
//~ EXIT 0

import std.io;

fn label[&i, &s](io: &!i Io, name: &s [byte], value: int) -> [io_write] int {
    io.write_all(io, name);
    io.space(io);
    io.print_int(io, value);
    io.newline(io);
    return 0;
}

// Pure by §2 of `docs/purity.md` — the row is `[]` and the parameter is
// a value — so §3 lets a call on a constant be evaluated.
fn factorial(n: int) -> [] int {
    if n < 2 {
        return 1;
    }
    return n * factorial(n - 1);
}

// Recursion is the case C's optimiser gives up on (§7): clang inlines
// and folds `factorial(5)`, and emits a runtime call for a recursive
// `fib`. This one folds.
fn fib(n: int) -> [] int {
    if n < 2 {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);

    borrow mut io as &!i in {
        // §2: three operators and three overflow checks before this
        // slice; one `xor` after it.
        label(i, "arithmetic", 2 + 3 * 4 - 14);
        // A call on constants, and a call on the answer to one.
        label(i, "nested", factorial(5));
        label(i, "recursion", fib(10));
        // The bit operators fold too — they are the ones most often
        // written as an expression rather than as a number.
        label(i, "bits", (1 << 8) - 1);
        label(i, "comparison", flag(1 < 2));

        // And the same computation with a value the compiler cannot
        // know, which is left alone and runs. Printing both is how this
        // fixture says that folding changed the *cost* and not the
        // answer.
        var ten = 0;
        ten = ten + 10;
        label(i, "runtime", fib(ten));
    }

    release(io);
    return 0;
}

fn flag(b: bool) -> [] int {
    if b {
        return 1;
    }
    return 0;
}
