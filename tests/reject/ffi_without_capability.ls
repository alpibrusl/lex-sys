//~ ERROR holds no capability that authorises it

// §8.4: the capability is the only way to reach a foreign call.
//
// The declaration is the place that rule has to be enforced, because it is
// the only place a foreign signature is written. A declaration that named
// the effect without taking the capability would be a door into C with no
// lock on it -- every caller's row would say `ffi("libc")` and not one of
// them would have needed authority to get there.

extern fn labs(n: int) -> [ffi("libc")] int;

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // This program touches no files, so that authority ends here.
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    return labs(0 - 7) - 7;
}
