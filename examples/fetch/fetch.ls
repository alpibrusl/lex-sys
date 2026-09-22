// `fetch` -- an HTTP client, and the first program here that connects.
//
//     fetch <address> <port> <path>
//
// Sends `GET <path>` to an IPv4 address and writes the response body to
// standard output. A 2xx status exits 0; any other status still writes
// the body and exits 1, with the status line on standard error. It is
// `curl -s` with one method, one protocol version and no names.
//
// `docs/net.md` §5 counted the programs that ask for each half of the
// network and found inbound 1, outbound 0 -- and said the next step was
// not to build `Net` but to write this, because a program that connects
// is the only way to find out what `connect` needs that the design had
// not thought of. `docs/connect.md` is what it found. Three things, each
// visible below where it happens:
//
// - **No names.** The first argument is `127.0.0.1`, never `localhost`.
//   `getaddrinfo` answers a pointer, and a foreign result is a scalar
//   (`reach.md` §3.1), so this program cannot resolve a host at all.
// - **The destination is data.** The address comes from `argv`, so no
//   row written at compile time can name it.
// - **The address is portable by accident.** `struct sockaddr_in` is
//   not the same bytes on Linux and on macOS. The Linux bytes work on
//   both only because macOS reads family 0 as `AF_INET` for
//   compatibility (`docs/connect.md` §3).
//
// Like `examples/serve/`, it is `extern fn` declarations against libc
// through `Ffi("libc")`, and its authority report says so and no more.

import std.bytes;
import std.io;

// ---------------------------------------------------------------------
// libc
// ---------------------------------------------------------------------

extern fn socket[&f](ffi: &f Ffi("libc"), domain: int, kind: int, proto: int)
    -> [ffi("libc")] int;

// The address crosses as a pointer and a length, the way `bind` does in
// `examples/serve/`: the slice is the `struct sockaddr_in` and its length
// is the `socklen_t`.
extern fn connect[&f, &a](ffi: &f Ffi("libc"), fd: int, addr: &a [byte])
    -> [ffi("libc")] int;

extern fn read[&f, &b](ffi: &f Ffi("libc"), fd: int, buf: &!b [byte])
    -> [ffi("libc")] int;

extern fn write[&f, &b](ffi: &f Ffi("libc"), fd: int, buf: &b [byte])
    -> [ffi("libc")] int;

extern fn close[&f](ffi: &f Ffi("libc"), fd: int) -> [ffi("libc")] int;

// ---------------------------------------------------------------------
// The command line
// ---------------------------------------------------------------------

// Four decimal octets separated by dots, into `out[0..4]`. Anything else
// is refused: a name, a fifth octet, an octet over 255, an empty one.
// This is the whole of this program's "resolver", and it is why it has
// no names.
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

// `struct sockaddr_in`, sixteen bytes, in the Linux layout.
//
// Linux:  `sa_family_t sin_family` is two bytes, little-endian: 2, 0.
// macOS:  `uint8_t sin_len` then `sa_family_t sin_family`, one byte
//         each: 16, 2.
//
// Then both agree: the port big-endian, the four octets, eight zeros.
// The first two bytes disagree, and Linux refuses the macOS bytes as
// family 528. The Linux bytes work on macOS only because BSD reads
// family 0 as `AF_INET` in both `bind` and `connect`, and takes
// `sin_len` from the length argument. So this array is portable by a
// compatibility rule, not by being right (`docs/connect.md` §3).
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

// A connected socket, or -1.
//
// -1 is all a failure can say: *why* it failed is in `errno`, a
// thread-local reached through a pointer (`docs/connect.md` §4), so
// "nothing is listening" and "no route to the host" look the same.
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

// Send the request, then read until the server closes: the header block
// into `head`, and every byte after the blank line straight to standard
// output. HTTP/1.0 with `Connection: close`, so the end of the body is the
// end of the stream and there is no length to trust.
//
// Answers the status, or -1 for a response that never finished its
// header block or did not start with a status line.
fn exchange[&f, &i, &h, &p](libc: &f Ffi("libc"), io: &!i Io, fd: int, host: &h [byte],
    path: &p [byte]) -> [ffi("libc"), io_write] int {
    region scratch {
        let request = alloc_slice[scratch](len(path) + len(host) + 64, byte_of(0));
        var at = put(request, 0, "GET ");
        at = put(request, at, path);
        at = put(request, at, " HTTP/1.0\r\nHost: ");
        at = put(request, at, host);
        at = put(request, at, "\r\nConnection: close\r\n\r\n");
        if !send_all(libc, fd, request[0..at]) {
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
                // Still in the header block. It may end anywhere in this
                // chunk, including across the boundary with the last one,
                // so the search runs over everything held so far -- and
                // only header bytes are held: the same read usually
                // carries the start of the body too, and those go
                // straight out from `chunk`. (The first version copied
                // the whole read into `head` and gave up when it did not
                // fit, which failed on any response whose first body
                // bytes arrived with its blank line.)
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
                    // Where the body starts, as an offset into this read.
                    // The terminator was not in `head[0..before]` or the
                    // last read would have found it, so this is past 0.
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
                if arg_count(g) != 4 {
                    io.error_all(i, "usage: fetch <address> <port> <path>\n");
                } else {
                    region scratch {
                        let octets = alloc_slice[scratch](4, byte_of(0));
                        let port = port_of(arg(g, 2));
                        if !octets_of(arg(g, 1), octets) {
                            io.error_all(i, "fetch: the address must be four decimal octets; there is no name resolution\n");
                        } else if port < 0 {
                            io.error_all(i, "fetch: the port must be 1..65535\n");
                        } else {
                            let fd = connect_to(f, octets, port);
                            if fd < 0 {
                                io.error_all(i, "fetch: could not connect\n");
                                status = 3;
                            } else {
                                let code = exchange(f, i, fd, arg(g, 1), arg(g, 3));
                                close(f, fd);
                                if code < 0 {
                                    io.error_all(i, "fetch: the response was not HTTP\n");
                                    status = 4;
                                } else if code >= 200 && code < 300 {
                                    status = 0;
                                } else {
                                    io.error_all(i, "fetch: the server answered with a non-2xx status\n");
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
