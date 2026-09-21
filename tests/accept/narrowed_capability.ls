// §7.4 and §8.4: attenuation, and a foreign call that fits inside what the
// attenuated capability authorises.
//
// `split` hands out an `Ffi` that names no library at all. It is authority
// over nothing until it is narrowed, and narrowing is the only way to get
// one that names something — there is no other constructor, exactly as
// there is none for `Io` (§8.2).
//
// Read `magnitude`'s row. `[ffi("libc"), io_write]` is the whole story of that
// function: it calls into one named library and it writes to the console,
// and a caller that holds neither capability cannot reach it.
//~ STDOUT 7
//~ EXIT 0

// libc's `long labs(long)`. The declaration is the only place this
// signature is written, and the capability parameter is the only way in.
extern fn labs[&f](ffi: &f Ffi("libc"), n: int) -> [ffi("libc")] int;

// Borrows both capabilities, so it declares both. The row is sorted by the
// label's text, which is why `ffi("libc")` comes first however it is written.
fn magnitude[&f, &i](ffi: &f Ffi("libc"), io: &!i Io, n: int) -> [io_write, ffi("libc")] int {
    let size = labs(ffi, n);
    putchar(io, 48 + size);
    putchar(io, 10);
    return size;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // This program touches no files, so that authority ends here.
    release(heap);
    release(fs);
    // The one narrowing in the program. `Ffi("")` becomes `Ffi("libc")`,
    // and nothing can turn it back: from here this capability reaches libc
    // and no other library, whatever the rest of the program does.
    let libc = narrow(ffi, "libc");

    var status = 0;
    borrow libc as &f in {
        borrow mut io as &!i in {
            status = magnitude(f, i, 0 - 7) - 7;
        }
    }

    // Both are resources, and both are destroyed exactly once. `main` owns
    // them, which is why its row is `[]` while all this happens.
    release(libc);
    release(io);
    return status;
}
