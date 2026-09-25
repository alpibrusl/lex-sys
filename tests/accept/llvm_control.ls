// The LLVM backend's third slice (`docs/llvm-backend.md` §5): control
// flow. `if`/`else`, `while`, all six comparisons and both short-circuit
// operators, each exercised for its value rather than only for compiling.
//
// Every local here is memory (one `alloca` per leaf) -- `if`/`while`
// need no `phi`, because the block after either one simply reads
// whatever the taken path last wrote, the same way a real register
// allocator would have rewritten it anyway.
//~ STDOUT 01X34Z
//~ STDOUT +-42
//~ STDOUT F
//~ STDOUT !T
//~ STDOUT T
//~ STDOUT !T
//~ EXIT 0

fn classify[&i](io: &!i Io, n: int) -> [io_write] int {
    if n > 100 {
        putchar(io, 43); // '+'
    } else {
        if n <= 0 {
            putchar(io, 45); // '-'
        } else {
            putchar(io, 48 + n / 10);
            putchar(io, 48 + n % 10);
        }
    }
    return 0;
}

// Impure (performs `io_write`), so a call to it is never folded away --
// which is exactly what proves the two short-circuit tests below skip
// calling it when they should: its own `!` either appears in the output
// or it does not.
fn shout[&i](io: &!i Io) -> [io_write] int {
    putchar(io, 33); // '!'
    return 1;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);

    borrow mut io as &!i in {
        var n = 0;
        while n < 5 {
            if n == 2 {
                putchar(i, 88); // 'X'
            } else {
                putchar(i, 48 + n);
            }
            n = n + 1;
        }
        // Proves the loop ran exactly five times, not merely that it
        // compiled: `n` is read back after the loop closed over it.
        if n != 5 {
            putchar(i, 89); // 'Y' -- should never print
        } else {
            putchar(i, 90); // 'Z'
        }
        putchar(i, 10);

        classify(i, 200);
        classify(i, 0 - 3);
        classify(i, 42);
        putchar(i, 10);

        // `&&`: lhs false, so `shout` must not run.
        if (1 < 0) && (shout(i) >= 1) {
            putchar(i, 84); // 'T'
        } else {
            putchar(i, 70); // 'F'
        }
        putchar(i, 10);
        // `&&`: lhs true, so `shout` must run.
        if (1 > 0) && (shout(i) >= 1) {
            putchar(i, 84);
        } else {
            putchar(i, 70);
        }
        putchar(i, 10);
        // `||`: lhs true, so `shout` must not run.
        if (1 > 0) || (shout(i) >= 1) {
            putchar(i, 84);
        } else {
            putchar(i, 70);
        }
        putchar(i, 10);
        // `||`: lhs false, so `shout` must run.
        if (1 < 0) || (shout(i) >= 1) {
            putchar(i, 84);
        } else {
            putchar(i, 70);
        }
        putchar(i, 10);
    }
    release(io);
    return 0;
}
