//~ STDOUT 42
//~ STDOUT 41
//~ STDOUT inner 1

// `docs/shadowing.md` §3: shadowing is allowed exactly when the shadowed
// binding is dead. Three shapes, one rule.
//
// 1. `val` shadows freely -- nothing is owed, so nothing can be
//    stranded. `n` below is built up in three steps under one name,
//    which is what the rule is for.
//
// 2. `res` shadows after it is consumed, and that works for a reason
//    that is not a special case: a `let`'s initialiser is lowered before
//    the binding exists, so `unbox(h, held)` has already consumed the
//    old `held` by the time the new one is declared. The same ordering
//    that makes `let x = x;` read the outer `x` or fail.
//
// 3. An *inner* block shadows a live binding freely, and always did. The
//    block ends first, so the outer binding is reachable again after it
//    and its own block-close check still covers it (§2.1). `held` is
//    live across the `if` here and is unboxed after it.

fn print_nat[&i](io: &!i Io, n: int) -> [io_write] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, 48 + n % 10);
}

fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io_write] int {
    var n = 0;
    while n < len(s) {
        putchar(io, int_of(s[n]));
        n = n + 1;
    }
    return len(s);
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);

    // 1. `val`, three steps, one name.
    let n = 4;
    let n = n * 10;
    let n = n + 2;

    var boxed = 0;
    var inner = 0;
    borrow mut heap as &!h in {
        // 2. `res`, consumed and then shadowed by what came out of it.
        let held = box(h, 41);
        let held = unbox(h, held);
        boxed = held;

        // 3. An inner block shadowing a binding that is still live.
        let live = box(h, 7);
        if boxed == 41 {
            let live = 1;
            inner = live;
        }
        boxed = boxed + unbox(h, live) - 7;
    }
    release(heap);

    borrow mut io as &!i in {
        print_nat(i, n);
        putchar(i, 10);
        print_nat(i, boxed);
        putchar(i, 10);
        write_all(i, "inner ");
        print_nat(i, inner);
        putchar(i, 10);
    }
    release(io);
    return n - 42;
}
