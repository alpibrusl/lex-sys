// `docs/filesystem.md` §1 and §4: narrowed once at the top, threaded down,
// used at the bottom.
//
// The capability here names a *file*, not a directory. `Fs("/tmp/x.txt")`
// is a perfectly ordinary point in the lattice -- prefix extension does not
// care whether what it names is a directory -- and what it authorises is
// one path and nothing else. The runtime check agrees: a path may be the
// granted name exactly, or continue from it at a `/`, and this capability
// has nothing below it to continue into.
//
// Read the rows on the way down. `store` says `fs_write("/tmp/lex-sys-narrowed.txt")`
// and so does everything that calls it, all the way to the `borrow` in
// `main`. Authority is visible in the types at every frame that carries it.
//~ STDOUT 5
//~ EXIT 0

fn print_nat[&i](io: &!i Io, n: int) -> [io_write] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, 48 + n % 10);
}

// The bottom of the chain: the only frame that actually performs the write.
fn store[&f, &r](
    fs: &f Fs("/tmp/lex-sys-narrowed.txt"),
    bytes: &r [byte],
) -> [fs_write("/tmp/lex-sys-narrowed.txt")] int {
    return fs_write(fs, "/tmp/lex-sys-narrowed.txt", bytes);
}

// The middle: it holds the capability only to pass it on, and its row says
// so anyway. A function that carries authority declares it whether it uses
// it itself or lends it to someone who does (§7.2).
fn save[&f](fs: &f Fs("/tmp/lex-sys-narrowed.txt")) -> [fs_write("/tmp/lex-sys-narrowed.txt")] int {
    return store(fs, "lines");
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // This program allocates nothing on the heap, so that authority ends here.
    release(heap);
    release(ffi);

    // The one narrowing in the program, and the whole of what the rest of
    // it may touch. `Fs("")` is authority over nothing until it names
    // somewhere, and from here it names one file for good.
    let one = narrow(fs, "/tmp/lex-sys-narrowed.txt");

    var wrote = 0;
    borrow one as &f in {
        wrote = save(f);
    }
    release(one);

    borrow mut io as &!i in {
        print_nat(i, wrote);
        putchar(i, 10);
    }
    release(io);
    return wrote - 5;
}
