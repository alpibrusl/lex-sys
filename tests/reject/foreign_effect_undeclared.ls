//~ ERROR does not declare

// §7.3 and §8.4: a foreign declaration's row is exact, like every other.
//
// This one holds the capability -- it cannot be called without authority --
// but it does not say what that authority is for. An empty row means pure,
// and a call into C is not pure however little it does. If this were
// allowed, `ffi("libc")` would vanish from every caller's signature above
// it and C's effects would be invisible again, which is the one thing §8.4
// exists to stop.

extern fn labs[&f](ffi: &f Ffi("libc"), n: int) -> [] int;

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // This program touches no files, so that authority ends here.
    release(heap);
    release(fs);
    release(io);
    let libc = narrow(ffi, "libc");
    var n = 0;
    borrow libc as &f in {
        n = labs(f, 0 - 7);
    }
    release(libc);
    return n - 7;
}
