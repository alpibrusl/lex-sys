//~ ERROR shared slice
//~ RULE shared-reference-written

// §3.1: an argument comes back *shared*, never unique.
//
// A program does not own its own command line. The bytes live in the
// memory the process was started with, they outlive every region in the
// program -- which is why the region is `static` -- and nothing here may
// write through one, the same way nothing may write through a string
// literal (`string_literal_is_shared.ls`).

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(ffi);
    release(fs);
    release(heap);
    release(io);

    borrow args as &a in {
        let first = arg(a, 0);
        first[0] = byte_of(65);
    }
    release(args);
    return 0;
}
