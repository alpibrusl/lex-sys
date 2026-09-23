// `docs/net.md` §4.1, `docs/connect.md` §1: slice 1 of `Net`
// (`docs/editions.md` §7). `connect` takes an address a caller already
// has -- four octets and a port, the same shape `examples/fetch/` builds
// by hand -- and there is no name to resolve yet.
//
// Read `probe`'s row: `net_out("127.0.0.1:1")` is the whole authority
// story, exactly as `roundtrip`'s `fs_read("/tmp")` is in
// `file_roundtrip.ls`. Nothing ever listens on port 1 of the loopback
// address, so this needs no server of its own and the connection is
// refused at once.
//~ STDOUT -1
//~ EXIT 0

edition 2;

import std.io;

// The row names the bound, exactly as `roundtrip`'s names a directory.
fn probe[&n, &i](
    net: &n Net("127.0.0.1:1"),
    io: &!i Io,
) -> [net_out("127.0.0.1:1"), io_write] int {
    let fd = connect(net, 127, 0, 0, 1, 1);
    io.print_int(io, fd);
    putchar(io, 10);
    return fd;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args, net } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // Nothing here calls into C directly, so that authority is dropped
    // at once.
    release(ffi);
    // This program touches no files, so that authority ends here.
    release(fs);
    // Nothing here allocates, so the heap ends here too.
    release(heap);

    let bound = narrow(net, "127.0.0.1:1");
    var fd = 0;
    borrow bound as &n in {
        borrow mut io as &!i in {
            fd = probe(n, i);
        }
    }
    release(bound);
    release(io);
    var status = 1;
    if fd < 0 {
        status = 0;
    }
    return status;
}
