//~ STDOUT hello

// `docs/function-values.md` §3: what this language writes instead of a
// function value, and what it costs.
//
// A program compiles whole, so the set of functions a "value" could name
// is always known. An enum names them and a `match` calls one. This is
// Reynolds' defunctionalization, it needs nothing the language lacks,
// and this is `std.io`'s `write_all` and `error_all` collapsed into one
// function the way a function value would have collapsed them.
//
// The cost is in the row. `write_to` performs whatever any arm performs,
// so every caller declares both streams, even one like `report` that
// only ever picks `Out`. `defunctionalized_row_is_the_union.ls` is the
// refusal that proves the caller cannot say less.

enum Stream { Out, Err }

fn write_to[&r, &i](io: &!i Io, which: Stream, s: &r [byte]) -> [io_write, err_write] int {
    match which {
        Stream::Out => {
            return write_bytes(io, s);
        }
        Stream::Err => {
            return write_err(io, s);
        }
    }
}

fn report[&i](io: &!i Io) -> [io_write, err_write] int {
    return write_to(io, Stream::Out, "hello\n");
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);
    var status = 0;
    borrow mut io as &!i in {
        status = report(i);
    }
    release(io);
    return 0;
}
