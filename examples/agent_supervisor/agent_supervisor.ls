// `agent_supervisor` -- the host side of `lex-os`'s guest/supervisor
// exchange, over plain HTTP/1.0 rather than `AF_VSOCK`.
//
//     agent_supervisor <port> <goal> <step>
//
// Binds the port named on the command line, accepts one connection,
// reads a `POST` request in full -- headers, then the body, by
// `Content-Length`, echoed to standard output the way `examples/collect/`
// already does -- and answers with an `AgentViewMsg` built from `<goal>`
// and `<step>`: `{"goal":"<goal>","step":<step>,"last_outcome":null,
// "completed":[],"reprovisions":0}`, checked byte for byte against real
// `serde_json` output for exactly that shape (a first step: no prior
// outcome, nothing completed yet, never reprovisioned).
//
// `examples/vsock/vsock.ls` plays the same exchange -- one `AgentViewMsg`
// out, one `AgentActionMsg` back -- over the real channel
// `lex-os-guest` uses, and says in its own comments that this sandbox
// has no `vhost_vsock`, so that round trip stays untested here. This
// program and `examples/agent_guest/` are not a second transport for
// `lex-os` -- `lex-os-proto` names no HTTP channel, and none is proposed
// here -- they exist to give the same view/action exchange a channel
// this sandbox *can* round-trip end to end, over a real socket, in real
// CI. The exchange is inverted from vsock's own shape only because HTTP
// is guest-initiated where a vsock stream lets the supervisor push
// first: `examples/agent_guest/` POSTs the action it would otherwise
// have sent last, and this program answers with the view it would
// otherwise have sent next.
//
// Like `examples/serve/` and `examples/collect/`, it is `extern fn`
// declarations against libc through `Ffi("libc")`, and its authority
// report says so and no more.

import std.bytes;
import std.io;

// ---------------------------------------------------------------------
// libc
// ---------------------------------------------------------------------

// `c_int`, not `int` (`docs/reach.md` §3.4): `socket`/`setsockopt`/
// `bind`/`listen`/`accept`/`close` all really return a 32-bit C `int`.
extern fn socket[&f](ffi: &f Ffi("libc"), domain: int, kind: int, proto: int)
    -> [ffi("libc")] c_int;

extern fn setsockopt[&f, &v](ffi: &f Ffi("libc"), fd: int, level: int,
    name: int, value: &v [byte]) -> [ffi("libc")] c_int;

extern fn bind[&f, &a](ffi: &f Ffi("libc"), fd: int, addr: &a [byte])
    -> [ffi("libc")] c_int;

extern fn listen[&f](ffi: &f Ffi("libc"), fd: int, backlog: int)
    -> [ffi("libc")] c_int;

extern fn accept[&f](ffi: &f Ffi("libc"), fd: int, addr: int, len: int)
    -> [ffi("libc")] c_int;

// `read`/`write` stay plain `int`: their real return is `ssize_t`,
// genuinely 64 bits here.
extern fn read[&f, &b](ffi: &f Ffi("libc"), fd: int, buf: &!b [byte])
    -> [ffi("libc")] int;

extern fn write[&f, &b](ffi: &f Ffi("libc"), fd: int, buf: &b [byte])
    -> [ffi("libc")] int;

extern fn close[&f](ffi: &f Ffi("libc"), fd: int) -> [ffi("libc")] c_int;

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
// a digit ends the number, the same rule `examples/serve/`'s `port_of`
// uses.
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

// ---------------------------------------------------------------------
// The wire protocol -- `AgentViewMsg`, fixed to the one shape a first
// step ever has (`docs`-worth of design left at `examples/vsock/
// vsock.ls`'s own header). Not a JSON library: one escaper, borrowed
// from that same file, and one encoder for exactly this shape.
// ---------------------------------------------------------------------

// Append `src` into `dst` at `at`, escaping `"`, `\`, and the three
// common control bytes -- copied from `examples/vsock/vsock.ls`'s
// `append_json_escaped`, needed here because `goal` crosses from argv
// with no guarantee it is already safe JSON content, and there is
// nowhere to put a shared function between two examples
// (`docs/many-files.md` is about a program's own files, not the
// corpus -- `examples/report/report.ls`'s own header makes the same
// point about `octets_of`).
fn put_escaped[&s, &d](dst: &!d [byte], at: int, src: &s [byte]) -> [] int {
    var out = at;
    var i = 0;
    while i < len(src) {
        let c = int_of(src[i]);
        if c == 34 {
            out = put(dst, out, "\\\"");
        } else if c == 92 {
            out = put(dst, out, "\\\\");
        } else if c == 10 {
            out = put(dst, out, "\\n");
        } else if c == 13 {
            out = put(dst, out, "\\r");
        } else if c == 9 {
            out = put(dst, out, "\\t");
        } else {
            dst[out] = src[i];
            out = out + 1;
        }
        i = i + 1;
    }
    return out;
}

// `{"goal":"<goal>","step":<step>,"last_outcome":null,"completed":[],
// "reprovisions":0}` -- checked against real `serde_json` output for
// `AgentViewMsg { goal, step, last_outcome: None, completed: vec![],
// reprovisions: 0 }`.
fn encode_view[&g, &d](dst: &!d [byte], goal: &g [byte], step: int) -> [] int {
    var at = put(dst, 0, "{\"goal\":\"");
    at = put_escaped(dst, at, goal);
    at = put(dst, at, "\",\"step\":");
    at = put_nat(dst, at, step);
    return put(dst, at, ",\"last_outcome\":null,\"completed\":[],\"reprovisions\":0}");
}

// ---------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------

// The value of `Content-Length:` inside a header block already known to
// hold one, or 0 -- copied from `examples/collect/collect.ls`.
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

// Read the one request this program ever answers -- headers, then the
// body by `Content-Length` -- writing the body (the guest's own
// `AgentActionMsg` line) to standard output as it arrives. Copied from
// `examples/collect/collect.ls`'s `read_request`, minus the connection
// count: this program accepts exactly one, the way `examples/serve/`
// does.
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

fn respond_with_view[&f, &g](libc: &f Ffi("libc"), conn: int, goal: &g [byte], step: int)
    -> [ffi("libc")] int {
    region scratch {
        let body = alloc_slice[scratch](len(goal) * 2 + 96, byte_of(0));
        let blen = encode_view(body, goal, step);
        let out = alloc_slice[scratch](blen + 96, byte_of(0));
        var at = put(out, 0, "HTTP/1.1 200 OK\r\nContent-Length: ");
        at = put_nat(out, at, blen);
        at = put(out, at, "\r\nConnection: close\r\nContent-Type: application/json\r\n\r\n");
        at = put(out, at, body[0..blen]);
        write(libc, conn, out[0..at]);
    }
    return 0;
}

// ---------------------------------------------------------------------
// The server
// ---------------------------------------------------------------------

fn run_supervisor[&f, &i, &g](libc: &f Ffi("libc"), io: &!i Io, port: int, goal: &g [byte],
    step: int) -> [ffi("libc"), io_write] int {
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
        respond_with_view(libc, conn, goal, step);
        close(libc, conn);
        close(libc, fd);
    }
    return 0;
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
                if arg_count(g) != 4 {
                    io.error_all(i, "usage: agent_supervisor <port> <goal> <step>\n");
                } else {
                    let port = nat_of(arg(g, 1));
                    let step = nat_of(arg(g, 3));
                    status = run_supervisor(f, i, port, arg(g, 2), step);
                }
            }
        }
    }
    release(libc);
    release(args);
    release(io);
    return status;
}
