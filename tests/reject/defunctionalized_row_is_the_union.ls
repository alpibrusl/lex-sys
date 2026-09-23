//~ ERROR `report` performs `err_write`, which its row [io_write] does not declare
//~ RULE effect-not-declared

// `docs/function-values.md` §3. The companion of
// `tests/accept/defunctionalized_stream.ls`, with one change: `report`
// declares only the stream it uses.
//
// It is refused, and correctly. The row is read off `write_to`'s
// declaration and never off the argument, so the enum that picked `Out`
// cannot narrow it: that would need the row to depend on a value, which
// is a dependent row and not something this checker does. With a
// function value whose type carried its row, the caller would name
// `write_bytes` and perform `[io_write]` exactly -- but only if
// `write_to` itself could be generic over the row, and that is row
// polymorphism (`effect-polymorphism.md`), not function values.

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

fn report[&i](io: &!i Io) -> [io_write] int {
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
