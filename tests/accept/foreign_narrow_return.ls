// `docs/reach.md` §3.4: `c_int` crosses a foreign return at the real C
// ABI width (32 bits) and sign-extends, rather than reading the full
// 64-bit return register the way a plain `int` return does. `access`'s
// real C return is `int`, and a failure is `-1` -- read as a plain
// `int` on this backend's first draft of `c_int`'s own motivating
// example (`examples/vsock/`), a real `connect` failure came back as
// `4294967295`, not `-1`, because the upper 32 bits of the return
// register were zero rather than sign-extended.
//~ STDOUT ok
//~ STDOUT -1
//~ EXIT 0

import std.io;

// `access` wants a NUL-terminated `char *`; a `&r [byte]` slice carries
// no NUL of its own (`docs/strings.md`), so both calls below append one
// by hand -- the same thing `checked_path`'s own NUL-termination does
// internally for `Fs`.
extern fn access[&f, &p](ffi: &f Ffi("libc"), path: &p [byte], mode: int)
    -> [ffi("libc")] c_int;

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(fs);
    release(heap);
    release(args);

    let libc = narrow(ffi, "libc");
    borrow mut io as &!i in {
        borrow libc as &f in {
            region scratch {
                // `F_OK` (0): the current directory exists.
                let here = alloc_slice[scratch](2, byte_of(0));
                here[0] = byte_of('.');
                if access(f, here, 0) == 0 {
                    io.write_all(i, "ok\n");
                } else {
                    io.write_all(i, "unexpected\n");
                }

                // A path nothing creates. `-1`, exactly -- not
                // `4294967295`, which is what a plain `int` return read
                // here before this slice, the upper 32 bits of the
                // register read as though they were part of the value
                // rather than left undefined by a real 32-bit C `int`'s
                // ABI.
                let text = "/no/such/path/at/all/lex-sys-probe";
                let missing = alloc_slice[scratch](len(text) + 1, byte_of(0));
                var j = 0;
                while j < len(text) {
                    missing[j] = text[j];
                    j = j + 1;
                }
                io.print_int(i, access(f, missing, 0));
                io.newline(i);
            }
        }
    }
    release(libc);
    release(io);
    return 0;
}
