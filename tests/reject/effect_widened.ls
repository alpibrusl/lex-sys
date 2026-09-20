//~ ERROR cannot be narrowed to

// §7.4: narrowing only, in both directions. The same commitment `lex-os`
// makes for manifests, for the same reason -- a program must not be able to
// grant itself what it was not given.
//
// Refinement is prefix extension, so `Ffi("libc")` is *narrower* than
// `Ffi("libcrypto")`: everything whose name starts with `libcrypto` starts
// with `libc` too. Going back up that chain is widening, and widening is
// the one direction attenuation does not have.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs } = split(world);
    // This program touches no files, so that authority ends here.
    release(fs);
    release(io);

    let crypto = narrow(ffi, "libcrypto");
    // Authority over one library, asking to become authority over every
    // library whose name it happens to extend.
    let wider = narrow(crypto, "libc");
    release(wider);
    return 0;
}
