// `report` -- an agent that posts its result, and the second program here
// that connects.
//
//     report <address> <port> <path> <message>
//
// Sends `POST <path> HTTP/1.0` to an IPv4 address, with `<message>` as
// the body, and writes the response body to standard output. Exit codes
// mirror `examples/fetch/`: 0 for a 2xx status, 1 for any other, 2 for a
// usage error, 3 when nothing accepts the connection, 4 for a response
// that is not HTTP.
//
// `docs/net.md` §5 and `docs/connect.md` §6 counted one asker for each
// half of the network and put the bar at two. `examples/fetch/` is the
// first outbound asker; this is the second, and a different shape of
// program on purpose -- not a generic client but the thing a run of any
// kind eventually needs, a way to tell someone what it produced. It
// reuses everything `docs/connect.md` already settled about the
// outbound half unchanged: no names (`octets_of` below is copied
// verbatim), the destination as run-time data, the Linux `struct
// sockaddr_in` layout that connects on both targets (§3), and the same
// opaque `errno` (§4). What it adds is a request with a body: the
// `Content-Length` this writes going *out* is the same assembly
// `examples/serve/`'s responses already do coming *back*, now exercised
// symmetrically on both sides of one connection.
//
// Like `examples/fetch/`, it is `extern fn` declarations against libc
// through `Ffi("libc")`, and its authority report says so and no more.

import std.bytes;
import std.io;

// ---------------------------------------------------------------------
// libc
// ---------------------------------------------------------------------

// `c_int`, not `int` (`docs/reach.md` §3.4): `socket`/`connect`/`close`
// all really return a 32-bit C `int`.
extern fn socket[&f](ffi: &f Ffi("libc"), domain: int, kind: int, proto: int)
    -> [ffi("libc")] c_int;

extern fn connect[&f, &a](ffi: &f Ffi("libc"), fd: int, addr: &a [byte])
    -> [ffi("libc")] c_int;

// `read`/`write` stay plain `int`: their real return is `ssize_t`,
// genuinely 64 bits here.
extern fn read[&f, &b](ffi: &f Ffi("libc"), fd: int, buf: &!b [byte])
    -> [ffi("libc")] int;

extern fn write[&f, &b](ffi: &f Ffi("libc"), fd: int, buf: &b [byte])
    -> [ffi("libc")] int;

extern fn close[&f](ffi: &f Ffi("libc"), fd: int) -> [ffi("libc")] c_int;

// ---------------------------------------------------------------------
// The command line
// ---------------------------------------------------------------------

// Four decimal octets separated by dots, into `out[0..4]`. Copied from
// `examples/fetch/fetch.ls` unchanged: `docs/connect.md` §1 already
// settled that there is no name resolution here, and a second program
// asking the same question gets the same answer.
fn octets_of[&t, &o](text: &t [byte], out: &!o [byte]) -> [] bool {
    var octet = 0;
    var digits = 0;
    var filled = 0;
    var i = 0;
    while i < len(text) {
        let c = int_of(text[i]);
        if c == '.' {
            if digits == 0 || filled == 3 {
                return false;
            }
            out[filled] = byte_of(octet);
            filled = filled + 1;
            octet = 0;
            digits = 0;
        } else {
            let digit = bytes.digit_of(c);
            if digit < 0 || digits == 3 {
                return false;
            }
            octet = octet * 10 + digit;
            if octet > 255 {
                return false;
            }
            digits = digits + 1;
        }
        i = i + 1;
    }
    if digits == 0 || filled != 3 {
        return false;
    }
    out[3] = byte_of(octet);
    return true;
}

// A port in `1..65536`, or -1.
fn port_of[&a](text: &a [byte]) -> [] int {
    if len(text) == 0 || len(text) > 5 {
        return 0 - 1;
    }
    var value = 0;
    var i = 0;
    while i < len(text) {
        let digit = bytes.digit_of(int_of(text[i]));
        if digit < 0 {
            return 0 - 1;
        }
        value = value * 10 + digit;
        i = i + 1;
    }
    if value < 1 || value > 65535 {
        return 0 - 1;
    }
    return value;
}

// ---------------------------------------------------------------------
// The address
// ---------------------------------------------------------------------

// `struct sockaddr_in`, sixteen bytes, in the Linux layout that connects
// on both targets (`docs/connect.md` §3). Copied from `fetch.ls`.
fn address[&o, &a](out: &!a [byte], octets: &o [byte], port: int) -> [] int {
    out[0] = byte_of(2);
    out[1] = byte_of(0);
    out[2] = byte_of(port / 256);
    out[3] = byte_of(port % 256);
    var i = 0;
    while i < 4 {
        out[4 + i] = octets[i];
        i = i + 1;
    }
    return 0;
}

// A connected socket, or -1. `errno` is a pointer (`docs/connect.md`
// §4), so -1 is all a failure here can say.
fn connect_to[&f, &o](libc: &f Ffi("libc"), octets: &o [byte], port: int)
    -> [ffi("libc")] int {
    region scratch {
        let addr = alloc_slice[scratch](16, byte_of(0));
        let fd = socket(libc, 2, 1, 0);
        if fd < 0 {
            return 0 - 1;
        }
        address(addr, octets, port);
        if connect(libc, fd, addr) == 0 {
            return fd;
        }
        close(libc, fd);
    }
    return 0 - 1;
}

// ---------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------

fn put[&s, &d](dst: &!d [byte], at: int, src: &s [byte]) -> [] int {
    var i = 0;
    while i < len(src) {
        dst[at + i] = src[i];
        i = i + 1;
    }
    return at + len(src);
}

// A decimal integer, written forwards by reversing the digits in place
// -- the same `Content-Length` trick `examples/serve/`'s `put_nat` uses
// for a response, now needed for a request.
fn put_nat[&d](dst: &!d [byte], at: int, n: int) -> [] int {
    if n == 0 {
        dst[at] = byte_of('0');
        return at + 1;
    }
    var rest = n;
    var end = at;
    while rest > 0 {
        dst[end] = byte_of('0' + rest - (rest / 10) * 10);
        rest = rest / 10;
        end = end + 1;
    }
    var lo = at;
    var hi = end - 1;
    while lo < hi {
        let swap = dst[lo];
        dst[lo] = dst[hi];
        dst[hi] = swap;
        lo = lo + 1;
        hi = hi - 1;
    }
    return end;
}

// Write all of `bytes`, which one `write` on a socket does not promise.
fn send_all[&f, &b](libc: &f Ffi("libc"), fd: int, data: &b [byte]) -> [ffi("libc")] bool {
    var sent = 0;
    while sent < len(data) {
        let n = write(libc, fd, data[sent..len(data)]);
        if n <= 0 {
            return false;
        }
        sent = sent + n;
    }
    return true;
}

// The status code from `HTTP/1.x NNN ...`, or -1.
fn status_of[&h](head: &h [byte]) -> [] int {
    if len(head) < 12 || !bytes.starts_with(head, "HTTP/1.") || int_of(head[8]) != ' ' {
        return 0 - 1;
    }
    var code = 0;
    var i = 9;
    while i < 12 {
        let digit = bytes.digit_of(int_of(head[i]));
        if digit < 0 {
            return 0 - 1;
        }
        code = code * 10 + digit;
        i = i + 1;
    }
    return code;
}

// Send `POST <path>` with `message` as its body, then read until the
// server closes: the header block into `head`, and every byte after the
// blank line straight to standard output. The request assembly differs
// from `fetch.ls`'s `exchange` in one way that matters beyond the
// `Content-Length` itself -- the header block and the body are sent as
// two `send_all` calls rather than one, because a region is a single
// 64 KiB arena chunk (`docs/defined-behaviour.md`) and a message from
// `argv` can be longer than that. Copying it into a scratch slice first
// would trap on any message over roughly 57 KiB; sending it straight
// from the caller's own slice has no such ceiling, the way `fetch.ls`
// already streams a response body without ever materialising the whole
// of it.
//
// `head_out` -- the header block alone -- is sized `len(path) +
// len(host) + 96`. The first version wrote `+ 64`, which is enough for
// the 63 bytes of literal text around it but leaves only one byte for
// `Content-Length`'s digits, so it wrote past the end of `head_out`
// -- caught by the bounds check the moment a body reached five
// figures, which every message this program is for actually does
// (`docs/connect.md` §8). The response side is unchanged, copied
// rather than shared because there is nowhere to put a shared function
// between two examples (`docs/many-files.md` is about a program's own
// files, not the corpus).
fn exchange[&f, &i, &h, &p, &m](libc: &f Ffi("libc"), io: &!i Io, fd: int, host: &h [byte],
    path: &p [byte], message: &m [byte]) -> [ffi("libc"), io_write] int {
    region scratch {
        let head_out = alloc_slice[scratch](len(path) + len(host) + 96, byte_of(0));
        var at = put(head_out, 0, "POST ");
        at = put(head_out, at, path);
        at = put(head_out, at, " HTTP/1.0\r\nHost: ");
        at = put(head_out, at, host);
        at = put(head_out, at, "\r\nContent-Length: ");
        at = put_nat(head_out, at, len(message));
        at = put(head_out, at, "\r\nConnection: close\r\n\r\n");
        if !send_all(libc, fd, head_out[0..at]) || !send_all(libc, fd, message) {
            return 0 - 1;
        }

        let head = alloc_slice[scratch](4096, byte_of(0));
        let chunk = alloc_slice[scratch](4096, byte_of(0));
        var held = 0;
        var status = 0 - 1;
        var body = false;
        var going = true;
        while going {
            let got = read(libc, fd, chunk);
            if got <= 0 {
                going = false;
            } else if body {
                io.write_all(io, chunk[0..got]);
            } else {
                let before = held;
                var take = got;
                if take > len(head) - held {
                    take = len(head) - held;
                }
                put(head, held, chunk[0..take]);
                held = held + take;
                let end = bytes.find(head[0..held], "\r\n\r\n");
                if end >= 0 {
                    status = status_of(head[0..held]);
                    body = true;
                    let from = end + 4 - before;
                    if from < got {
                        io.write_all(io, chunk[from..got]);
                    }
                } else if held == len(head) {
                    return 0 - 1;
                }
            }
        }
        if !body {
            return 0 - 1;
        }
        return status;
    }
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // A client: no files and no heap, and nothing downstream can get
    // either back.
    release(fs);
    release(heap);

    let libc = narrow(ffi, "libc");
    var status = 2;
    borrow mut io as &!i in {
        borrow libc as &f in {
            borrow args as &g in {
                if arg_count(g) != 5 {
                    io.error_all(i, "usage: report <address> <port> <path> <message>\n");
                } else {
                    region scratch {
                        let octets = alloc_slice[scratch](4, byte_of(0));
                        let port = port_of(arg(g, 2));
                        if !octets_of(arg(g, 1), octets) {
                            io.error_all(i, "report: the address must be four decimal octets; there is no name resolution\n");
                        } else if port < 0 {
                            io.error_all(i, "report: the port must be 1..65535\n");
                        } else {
                            let fd = connect_to(f, octets, port);
                            if fd < 0 {
                                io.error_all(i, "report: could not connect\n");
                                status = 3;
                            } else {
                                let code = exchange(f, i, fd, arg(g, 1), arg(g, 3), arg(g, 4));
                                close(f, fd);
                                if code < 0 {
                                    io.error_all(i, "report: the response was not HTTP\n");
                                    status = 4;
                                } else if code >= 200 && code < 300 {
                                    status = 0;
                                } else {
                                    io.error_all(i, "report: the server answered with a non-2xx status\n");
                                    status = 1;
                                }
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
