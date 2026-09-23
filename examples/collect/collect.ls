// `collect` -- an agent that receives results, and the second program
// here that listens.
//
//     collect <port> <count>
//
// Binds the port named on the command line, accepts `<count>`
// connections one after another, and for each one reads a `POST`
// request in full -- headers, then the body, by `Content-Length` --
// writing the body to standard output before answering `200` and
// moving on. Exits 0 once `<count>` requests have been handled.
//
// `docs/listen.md` is the design. `examples/serve/` accepts exactly one
// connection and never reads a body, which is what makes it testable
// without a shutdown protocol; this program keeps that discipline with
// a number instead of the constant one, and adds the one thing `serve/`
// never needed: reading a body it does not already have the length of
// in one read. The body streams straight to standard output the way
// `examples/fetch/` and `examples/report/` stream a response, rather
// than being copied into one buffer first -- a region is a single
// 64 KiB arena chunk (`docs/defined-behaviour.md`), and `docs/connect.md`
// §8 already found what materialising a whole body costs.
//
// Like `serve/`, it is `extern fn` declarations against libc through
// `Ffi("libc")`, and its authority report says so and no more.

import std.bytes;
import std.io;

// ---------------------------------------------------------------------
// libc
// ---------------------------------------------------------------------

extern fn socket[&f](ffi: &f Ffi("libc"), domain: int, kind: int, proto: int)
    -> [ffi("libc")] int;

extern fn setsockopt[&f, &v](ffi: &f Ffi("libc"), fd: int, level: int,
    name: int, value: &v [byte]) -> [ffi("libc")] int;

extern fn bind[&f, &a](ffi: &f Ffi("libc"), fd: int, addr: &a [byte])
    -> [ffi("libc")] int;

extern fn listen[&f](ffi: &f Ffi("libc"), fd: int, backlog: int)
    -> [ffi("libc")] int;

extern fn accept[&f](ffi: &f Ffi("libc"), fd: int, addr: int, len: int)
    -> [ffi("libc")] int;

extern fn read[&f, &b](ffi: &f Ffi("libc"), fd: int, buf: &!b [byte])
    -> [ffi("libc")] int;

extern fn write[&f, &b](ffi: &f Ffi("libc"), fd: int, buf: &b [byte])
    -> [ffi("libc")] int;

extern fn close[&f](ffi: &f Ffi("libc"), fd: int) -> [ffi("libc")] int;

// ---------------------------------------------------------------------
// Bytes
// ---------------------------------------------------------------------

fn put[&s, &d](dst: &!d [byte], at: int, src: &s [byte]) -> [] int {
    var i = 0;
    while i < len(src) {
        dst[at + i] = src[i];
        i = i + 1;
    }
    return at + len(src);
}

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

// A base-ten value, as the command line spells it. Anything that is not
// a digit ends the number, the same rule `serve.ls`'s `port_of` uses.
fn nat_of[&a](text: &a [byte]) -> [] int {
    var value = 0;
    var i = 0;
    while i < len(text) {
        let digit = bytes.digit_of(int_of(text[i]));
        if digit < 0 {
            return value;
        }
        value = value * 10 + digit;
        i = i + 1;
    }
    return value;
}

// The value of `Content-Length:` inside a header block already known to
// hold one, or 0 if there is none -- a request with a body always
// declares one here, and one without a body is treated as empty rather
// than refused, so `collect` accepts a bare `POST` too.
fn content_length_of[&h](head: &h [byte]) -> [] int {
    let at = bytes.find(head, "Content-Length: ");
    if at < 0 {
        return 0;
    }
    var i = at + len("Content-Length: ");
    var value = 0;
    while i < len(head) {
        let digit = bytes.digit_of(int_of(head[i]));
        if digit < 0 {
            return value;
        }
        value = value * 10 + digit;
        i = i + 1;
    }
    return value;
}

// ---------------------------------------------------------------------
// The connection
// ---------------------------------------------------------------------

// Read one request in full and write its body to standard output,
// streamed rather than materialised. Answers `true` once a complete
// header block was found and its body, if any, fully read; `false` for
// a connection that closed before either.
//
// The shape mirrors `examples/report/`'s `exchange`, reading a status
// line: hold header bytes in `head` until the blank line, then forward
// everything after it. What is new here is that a fixed byte count
// -- `Content-Length`, not "until the peer closes" -- decides when this
// is done, since a client on a real connection (this program's own
// tests included) may keep it open past the body it sent.
fn read_request[&f, &i](libc: &f Ffi("libc"), io: &!i Io, conn: int) -> [ffi("libc"), io_write] bool {
    region scratch {
        let head = alloc_slice[scratch](4096, byte_of(0));
        let chunk = alloc_slice[scratch](4096, byte_of(0));
        var held = 0;
        var body_total = 0 - 1;
        var body_seen = 0;
        var done = false;
        var going = true;
        while going {
            let got = read(libc, conn, chunk);
            if got <= 0 {
                going = false;
            } else if body_total >= 0 {
                io.write_all(io, chunk[0..got]);
                body_seen = body_seen + got;
                if body_seen >= body_total {
                    done = true;
                    going = false;
                }
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
                    body_total = content_length_of(head[0..held]);
                    let from = end + 4 - before;
                    if from < got {
                        io.write_all(io, chunk[from..got]);
                        body_seen = got - from;
                    }
                    if body_total == 0 || body_seen >= body_total {
                        done = true;
                        going = false;
                    }
                } else if held == len(head) {
                    going = false;
                }
            }
        }
        return done;
    }
}

fn respond[&f](libc: &f Ffi("libc"), conn: int) -> [ffi("libc")] int {
    region scratch {
        let body = "{\"ok\":true}";
        let out = alloc_slice[scratch](len(body) + 96, byte_of(0));
        var at = put(out, 0, "HTTP/1.1 200 OK\r\nContent-Length: ");
        at = put_nat(out, at, len(body));
        at = put(out, at, "\r\nConnection: close\r\nContent-Type: application/json\r\n\r\n");
        at = put(out, at, body);
        write(libc, conn, out[0..at]);
    }
    return 0;
}

// ---------------------------------------------------------------------
// The server
// ---------------------------------------------------------------------

fn collect[&f, &i](libc: &f Ffi("libc"), io: &!i Io, port: int, count: int)
    -> [ffi("libc"), io_write] int {
    region scratch {
        let fd = socket(libc, 2, 1, 0);
        if fd < 0 {
            return 1;
        }
        let enable = alloc_slice[scratch](4, byte_of(0));
        enable[0] = byte_of(1);
        setsockopt(libc, fd, 1, 2, enable);

        let addr = alloc_slice[scratch](16, byte_of(0));
        addr[0] = byte_of(2);
        addr[2] = byte_of(port / 256);
        addr[3] = byte_of(port - (port / 256) * 256);
        if bind(libc, fd, addr) < 0 {
            close(libc, fd);
            return 2;
        }
        listen(libc, fd, 16);

        var handled = 0;
        while handled < count {
            let conn = accept(libc, fd, 0, 0);
            if conn < 0 {
                close(libc, fd);
                return 3;
            }
            if !read_request(libc, io, conn) {
                close(libc, conn);
                close(libc, fd);
                return 4;
            }
            respond(libc, conn);
            close(libc, conn);
            handled = handled + 1;
        }
        close(libc, fd);
    }
    return 0;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(fs);
    release(heap);

    let libc = narrow(ffi, "libc");
    var status = 5;
    borrow mut io as &!i in {
        borrow libc as &f in {
            borrow args as &g in {
                if arg_count(g) > 2 {
                    let port = nat_of(arg(g, 1));
                    let count = nat_of(arg(g, 2));
                    status = collect(f, i, port, count);
                }
            }
        }
    }
    release(libc);
    release(args);
    release(io);
    return status;
}
