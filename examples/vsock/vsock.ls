// `vsock` -- connecting over `AF_VSOCK`, the channel `lex-os-guest` uses to
// reach its host supervisor (`crates/lex-os-proto/src/vsock.rs` in
// `lex-os`, the runtime this program is scoping a port of).
//
//     vsock <cid> <port>
//
// Opens an `AF_VSOCK` socket and connects to `(cid, port)`, reporting what
// happened. Like `examples/serve/` and `examples/fetch/`, this is `extern
// fn` against libc through `Ffi("libc")` -- `AF_VSOCK` is a Linux-specific
// address family with no `Net` builtin behind it, and `docs/reach.md`'s
// whole point is that it does not need one: what decides whether a
// program is writable here is not a feature list, it is whether the
// authority it needs has a name, and libc already has one.
//
// `struct sockaddr_vm` (Linux's `<linux/vm_sockets.h>`) is 16 bytes:
// `svm_family` (`u16`) at 0, `svm_reserved1` (`u16`, must be 0) at 2,
// `svm_port` (`u32`) at 4, `svm_cid` (`u32`) at 8, then 4 zero bytes --
// checked directly against the real header (`offsetof`, a small C
// probe) rather than assumed. Unlike `struct sockaddr_in`'s port,
// `svm_port`/`svm_cid` are **host** byte order, not network byte order,
// so there is no endian flip to get right on these little-endian targets
// -- one thing this layout does not share with `examples/serve/`'s.

import std.bytes;
import std.io;

extern fn socket[&f](ffi: &f Ffi("libc"), domain: int, kind: int, proto: int)
    -> [ffi("libc")] int;

extern fn connect[&f, &a](ffi: &f Ffi("libc"), fd: int, addr: &a [byte])
    -> [ffi("libc")] int;

extern fn close[&f](ffi: &f Ffi("libc"), fd: int) -> [ffi("libc")] int;

// Little-endian, four bytes, host order -- what `svm_port`/`svm_cid` both
// want.
fn store_u32[&d](dst: &!d [byte], at: int, value: int) -> [] int {
    dst[at] = byte_of(value & 0xff);
    dst[at + 1] = byte_of((value >> 8) & 0xff);
    dst[at + 2] = byte_of((value >> 16) & 0xff);
    dst[at + 3] = byte_of((value >> 24) & 0xff);
    return 0;
}

// A decimal, unsigned, up to 32 bits, or -1 -- `svm_cid`/`svm_port` are
// both `u32`, wider than a TCP port's 16, so `examples/fetch/`'s own
// `port_of` (five digits) is not wide enough to reuse here.
fn u32_of[&a](text: &a [byte]) -> [] int {
    if len(text) == 0 || len(text) > 10 {
        return 0 - 1;
    }
    var n = 0;
    var i = 0;
    while i < len(text) {
        let digit = bytes.digit_of(int_of(text[i]));
        if digit < 0 {
            return 0 - 1;
        }
        n = n * 10 + digit;
        if n > 4294967295 {
            return 0 - 1;
        }
        i = i + 1;
    }
    return n;
}

// `-1` for a `socket` failure, `-2` for a `connect` failure (the fd is
// closed first), or the connected fd.
fn dial[&f, &a](libc: &f Ffi("libc"), addr: &a [byte]) -> [ffi("libc")] int {
    let fd = socket(libc, 40, 1, 0);
    if fd < 0 {
        return 0 - 1;
    }
    if connect(libc, fd, addr) < 0 {
        close(libc, fd);
        return 0 - 2;
    }
    return fd;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(fs);
    release(heap);

    let libc = narrow(ffi, "libc");
    var status = 2;
    borrow mut io as &!i in {
        borrow libc as &f in {
            borrow args as &g in {
                if arg_count(g) != 3 {
                    io.error_all(i, "usage: vsock <cid> <port>\n");
                } else {
                    let cid = u32_of(arg(g, 1));
                    let port = u32_of(arg(g, 2));
                    if cid < 0 || port < 0 {
                        io.error_all(i, "vsock: cid and port must be decimal, 0..4294967295\n");
                    } else {
                        region scratch {
                            let addr = alloc_slice[scratch](16, byte_of(0));
                            // `addr[1]` (family's high byte) and `addr[2..4]`
                            // (`svm_reserved1`) stay 0 -- `alloc_slice`'s own
                            // fill value, and `AF_VSOCK` (40) fits one byte.
                            addr[0] = byte_of(40);
                            store_u32(addr, 4, port);
                            store_u32(addr, 8, cid);

                            let fd = dial(f, addr);
                            if fd < 0 {
                                io.error_all(i, "vsock: could not connect\n");
                                status = 3;
                            } else {
                                io.write_all(i, "connected\n");
                                close(f, fd);
                                status = 0;
                            }
                        }
                    }
                }
            }
        }
    }
    release(libc);
    release(args);
    release(io);
    return status;
}
