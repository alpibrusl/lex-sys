// `docs/strings.md` §1 and §4: a string is a run of bytes, and a literal
// is a *shared* slice into the object file's read-only data.
//
// The type is `&static [byte]` -- an ordinary slice, so an ordinary
// reference, so `len` and `s[i]` and the bounds check all work on it with
// nothing added. The static region outlives everything, which is why a
// literal can be returned from a function while an arena's slice cannot.
//~ STDOUT Hello, world!
//~ STDOUT tab:	| newline done
//~ STDOUT 13 5
//~ EXIT 0

fn print_nat[&i](io: &!i Io, n: int) -> [io] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, 48 + n % 10);
}

// A byte at a time, which is all `putchar` can take. §6's `write` takes the
// whole slice at once; this shows the slice itself.
fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io] int {
    var n = 0;
    while n < len(s) {
        putchar(io, int_of(s[n]));
        n = n + 1;
    }
    return len(s);
}

// A literal may be *returned*, because `static` outlives every region a
// caller could have. An arena's slice may not -- see
// `tests/reject/string_escapes_its_region.ls`.
fn greeting() -> [] &static [byte] {
    return "Hello, world!\n";
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap } = split(world);
    // This program touches no files, so that authority ends here.
    release(heap);
    release(fs);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    var status = 0;
    borrow mut io as &!i in {
        let written = write_all(i, greeting());

        // The five escapes, and the only ones (§4).
        write_all(i, "tab:\t| newline done\n");

        // `len` is the byte length, not a character count: this design
        // claims no encoding, so there are no characters to count.
        print_nat(i, written - 1);
        putchar(i, 32);
        print_nat(i, len("hello"));
        putchar(i, 10);

        status = written - 14;
    }
    release(io);
    return status;
}
