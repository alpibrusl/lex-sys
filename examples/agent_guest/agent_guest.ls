// `agent_guest` -- the guest side of `lex-os`'s guest/supervisor
// exchange, over plain HTTP/1.0 rather than `AF_VSOCK`.
//
//     agent_guest <address> <port>
//
// POSTs `/step` with `{"action":"done"}` as the body and decodes the
// `AgentViewMsg` JSON it gets back, printing the goal and step it
// carries -- the same round `examples/vsock/vsock.ls`'s `converse` plays
// over a real `AF_VSOCK` socket, here over a real TCP one instead, and
// see that file's own header for why: this sandbox has no
// `vhost_vsock`, so this pair (`examples/agent_supervisor/` is the other
// half) exists to give the same exchange a channel testable end to end,
// in real CI, rather than to add a second transport `lex-os-proto` does
// not name. The exchange is inverted from vsock's own shape because
// HTTP is guest-initiated where a vsock stream lets the supervisor push
// first: this program POSTs the action it would otherwise have sent
// last, and `examples/agent_supervisor/` answers with the view it would
// otherwise have sent next.
//
// Exit codes mirror `examples/report/`: 0 for a 2xx status whose body
// decoded as a view, 1 for any other status, 2 for a usage error, 3
// when nothing accepts the connection, 4 for a response that is not
// HTTP or whose body does not decode.
//
// Everything about connecting -- octets, no name resolution, the Linux
// `struct sockaddr_in` layout, the opaque `errno` -- is copied from
// `examples/fetch/` and `examples/report/` unchanged; `docs/connect.md`
// already settled it and a third program asking the same question gets
// the same answer.

import std.bytes;
import std.io;

// ---------------------------------------------------------------------
// libc
// ---------------------------------------------------------------------

extern fn socket[&f](ffi: &f Ffi("libc"), domain: int, kind: int, proto: int)
    -> [ffi("libc")] c_int;

extern fn connect[&f, &a](ffi: &f Ffi("libc"), fd: int, addr: &a [byte])
    -> [ffi("libc")] c_int;

extern fn read[&f, &b](ffi: &f Ffi("libc"), fd: int, buf: &!b [byte])
    -> [ffi("libc")] int;

extern fn write[&f, &b](ffi: &f Ffi("libc"), fd: int, buf: &b [byte])
    -> [ffi("libc")] int;

extern fn close[&f](ffi: &f Ffi("libc"), fd: int) -> [ffi("libc")] c_int;

// ---------------------------------------------------------------------
// The command line -- copied from `examples/report/report.ls`
// ---------------------------------------------------------------------

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
// Bytes and the wire protocol
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

// The status code from `HTTP/1.x NNN ...`, or -1 -- copied from
// `examples/report/report.ls`.
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

// The `AgentViewMsg` decoder -- copied from `examples/vsock/vsock.ls`'s
// `find_after`/`end_of_quoted`/`goal_start_of`/`goal_end_of`/`step_of`,
// unchanged: the JSON shape crossing this wire is the exact same shape
// crossing that one, and there is nowhere to put a shared function
// between two examples (`docs/many-files.md`).
fn find_after[&hay, &needle](hay: &hay [byte], needle: &needle [byte], start: int) -> [] int {
    let hn = len(hay);
    let nn = len(needle);
    var i = start;
    while i + nn <= hn {
        var matched = true;
        var j = 0;
        while j < nn {
            if int_of(hay[i + j]) != int_of(needle[j]) {
                matched = false;
            }
            j = j + 1;
        }
        if matched {
            return i + nn;
        }
        i = i + 1;
    }
    return 0 - 1;
}

fn end_of_quoted[&s](s: &s [byte], start: int) -> [] int {
    var i = start;
    let n = len(s);
    while i < n {
        let c = int_of(s[i]);
        if c == 92 {
            i = i + 2;
        } else if c == 34 {
            return i;
        } else {
            i = i + 1;
        }
    }
    return 0 - 1;
}

fn goal_start_of[&line](line: &line [byte]) -> [] int {
    return find_after(line, "{\"goal\":\"", 0);
}

fn goal_end_of[&line](line: &line [byte], start: int) -> [] int {
    return end_of_quoted(line, start);
}

fn step_of[&line](line: &line [byte], after: int) -> [] int {
    let after_key = find_after(line, "\"step\":", after);
    if after_key < 0 {
        return 0 - 1;
    }
    var i = after_key;
    let n = len(line);
    var step = 0;
    var saw_digit = false;
    while i < n {
        let d = bytes.digit_of(int_of(line[i]));
        if d < 0 {
            i = n;
        } else {
            step = step * 10 + d;
            saw_digit = true;
            i = i + 1;
        }
    }
    if !saw_digit {
        return 0 - 1;
    }
    return step;
}

// ---------------------------------------------------------------------
// The exchange
// ---------------------------------------------------------------------

// Send `POST /step` with `{"action":"done"}` as its body, read until
// the server closes (HTTP/1.0, `Connection: close`), then decode the
// `AgentViewMsg` the response body carries and print its goal and step.
//
// Unlike `examples/report/`'s `exchange`, which streams a response body
// straight to standard output because it never needs to look inside
// it, this one must hold the whole response before decoding -- `128
// KiB` (two 64 KiB regions apart, `docs/defined-behaviour.md`) is far
// more than a demo `AgentViewMsg` line ever is, so a real one is never
// truncated by this ceiling.
fn exchange[&f, &i, &h](libc: &f Ffi("libc"), io: &!i Io, fd: int, host: &h [byte])
    -> [ffi("libc"), io_write] int {
    region scratch {
        let action = "{\"action\":\"done\"}";
        let head_out = alloc_slice[scratch](len(host) + 96, byte_of(0));
        var at = put(head_out, 0, "POST /step HTTP/1.0\r\nHost: ");
        at = put(head_out, at, host);
        at = put(head_out, at, "\r\nContent-Length: ");
        at = put_nat(head_out, at, len(action));
        at = put(head_out, at, "\r\nConnection: close\r\n\r\n");
        if !send_all(libc, fd, head_out[0..at]) || !send_all(libc, fd, action) {
            return 0 - 1;
        }

        let buf = alloc_slice[scratch](4096, byte_of(0));
        var held = 0;
        var going = true;
        while going {
            if held == len(buf) {
                going = false;
            } else {
                let got = read(libc, fd, buf[held..len(buf)]);
                if got <= 0 {
                    going = false;
                } else {
                    held = held + got;
                }
            }
        }

        let end = bytes.find(buf[0..held], "\r\n\r\n");
        if end < 0 {
            return 0 - 1;
        }
        let status = status_of(buf[0..held]);
        let body = buf[end + 4..held];

        let goal_start = goal_start_of(body);
        if goal_start < 0 {
            return 0 - 2;
        }
        let goal_end = goal_end_of(body, goal_start);
        let step = step_of(body, goal_start);
        if goal_end < 0 || step < 0 {
            return 0 - 2;
        }

        io.write_all(io, "goal: ");
        io.write_all(io, body[goal_start..goal_end]);
        io.write_all(io, "\n");
        let digits = alloc_slice[scratch](24, byte_of(0));
        var dat = put(digits, 0, "step: ");
        dat = put_nat(digits, dat, step);
        digits[dat] = byte_of(10);
        dat = dat + 1;
        io.write_all(io, digits[0..dat]);
        return status;
    }
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
                    io.error_all(i, "usage: agent_guest <address> <port>\n");
                } else {
                    region scratch {
                        let octets = alloc_slice[scratch](4, byte_of(0));
                        let port = port_of(arg(g, 2));
                        if !octets_of(arg(g, 1), octets) {
                            io.error_all(i, "agent_guest: the address must be four decimal octets; there is no name resolution\n");
                        } else if port < 0 {
                            io.error_all(i, "agent_guest: the port must be 1..65535\n");
                        } else {
                            let fd = connect_to(f, octets, port);
                            if fd < 0 {
                                io.error_all(i, "agent_guest: could not connect\n");
                                status = 3;
                            } else {
                                let code = exchange(f, i, fd, arg(g, 1));
                                close(f, fd);
                                if code == 0 - 1 {
                                    io.error_all(i, "agent_guest: the response was not HTTP\n");
                                    status = 4;
                                } else if code == 0 - 2 {
                                    io.error_all(i, "agent_guest: the view did not decode\n");
                                    status = 4;
                                } else if code >= 200 && code < 300 {
                                    status = 0;
                                } else {
                                    io.error_all(i, "agent_guest: the supervisor answered with a non-2xx status\n");
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
