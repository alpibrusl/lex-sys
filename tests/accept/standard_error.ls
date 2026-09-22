// `docs/standard-error.md`: a third stream under the same capability,
// with its own label.
//
// What this fixture holds is the *separation*. Every line below goes to
// exactly one stream, the harness checks both, and it checks the empty
// case too -- which is the half that matters, because the failure being
// guarded against is a diagnostic arriving where data was expected
// (§1.1, where that cost a `sort` downstream one bogus line).
//
// Per-stream order is the promise; across the two streams there is none
// (§5), so nothing here asserts an interleaving.
//~ STDOUT data one
//~ STDOUT data two
//~ STDOUT data three
//~ STDERR first complaint
//~ STDERR second complaint
//~ STDERR through std.io
//~ EXIT 0

import std.io;

// Two labels on one function, because it uses both streams. Neither is
// implied by the other.
fn both[&i](io: &!i Io) -> [io_write, err_write] int {
    io.write_all(io, "data two\n");
    return write_err(io, "second complaint\n");
}

// One label, because a diagnostic is all this does. `authority.md` §2.2:
// the absent `io_write` is a proof.
fn quiet_on_stdout[&i](io: &!i Io) -> [err_write] int {
    return io.error_all(io, "through std.io\n");
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);

    borrow mut io as &!i in {
        write_bytes(i, "data one\n");
        write_err(i, "first complaint\n");
        both(i);
        io.write_all(i, "data three\n");
        quiet_on_stdout(i);

        // An empty diagnostic writes nothing and is not an error, the
        // way an empty `write_bytes` is not (`bulk-io.md` §3).
        region a {
            let nothing = alloc_slice[a](0, byte_of(0));
            write_err(i, nothing);
        }
    }

    release(io);
    return 0;
}
