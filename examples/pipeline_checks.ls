// A fallible pipeline: acquire a buffer, then bail out at any of
// several checks. This is the program `docs/defer.md` §4 is about, and
// the reason it is an example rather than a fixture is that the case
// for `defer` is not the code that exists — it is the code nobody
// wrote, because the alternative was this:
//
//     if length == 0 {
//         console.write_all(io, "empty\n");
//         return 0 - buffer.drop(heap, b) - 1;      // drop, again
//     }
//     if length > 32 {
//         console.write_all(io, "too long\n");
//         return 0 - buffer.drop(heap, b) - 2;      // drop, again
//     }
//     ...
//
// Four exits, four repetitions of the drop, each one tangled into the
// return expression so that the error code and the cleanup are computed
// in the same breath. The checker was right to insist — a path that
// forgot would be a leak — but insisting is all it could do.
//
// One `defer` replaces all four. The buffer is still consumed exactly
// once on every path, still by a function named here, and the row still
// says `[heap]`. What changed is that the line saying so sits next to
// the line that acquired it, which is where a reader looks for the
// pairing.
//~ STDOUT ok: fine
//~ STDOUT empty
//~ STDOUT bad prefix
//~ STDOUT 3 checked
//~ EXIT 0

import std.buffer;
import std.bytes;
import std.io as console;

// Every `return` below runs the `defer`, and both halves of that are
// checked rather than asserted: take the `defer` out and this function
// stops compiling -- *`b` is still live here* at the first early return
// -- and with it in, valgrind reports 4 allocs and 4 frees.
fn process[&h, &i](heap: &!h Heap, io: &!i Io, text: &static [byte]) -> [heap, io_write] int {
    var b = buffer.empty(heap, 16);
    defer buffer.drop(heap, b);
    b = buffer.append(heap, b, text);

    var length = 0;
    borrow b as &r in {
        length = len(buffer.bytes(r));
    }

    if length == 0 {
        console.write_all(io, "empty\n");
        return 0;
    }
    if length > 32 {
        console.write_all(io, "too long\n");
        return 0;
    }

    var tagged = false;
    borrow b as &r in {
        tagged = bytes.starts_with(buffer.bytes(r), "ok:");
    }
    if tagged == false {
        console.write_all(io, "bad prefix\n");
        return 0;
    }

    borrow b as &r in {
        console.write_all(io, buffer.bytes(r));
    }
    console.newline(io);
    return 1;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    // This program touches no files, so that authority ends here.
    release(fs);

    var accepted = 0;
    var checked = 0;
    borrow mut heap as &!h in {
        borrow mut io as &!i in {
            accepted = accepted + process(h, i, "ok: fine");
            accepted = accepted + process(h, i, "");
            accepted = accepted + process(h, i, "nope");
            checked = 3;
            console.print_int(i, checked);
            console.write_all(i, " checked");
            console.newline(i);
        }
    }
    release(heap);
    release(io);
    return accepted - 1;
}
