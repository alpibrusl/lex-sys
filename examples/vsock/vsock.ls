// `vsock` -- connecting over `AF_VSOCK`, the channel `lex-os-guest` uses to
// reach its host supervisor (`crates/lex-os-proto/src/vsock.rs` in
// `lex-os`, the runtime this program is scoping a port of), and speaking
// one round of its wire protocol once connected: read one `AgentViewMsg`
// line, print the goal and step it carries, answer with a `Done` action.
//
//     vsock <cid> <port>
//
// Opens an `AF_VSOCK` socket, connects to `(cid, port)`, and if that
// succeeds, receives and sends exactly the newline-delimited JSON
// `StreamGuestTransport` itself reads and writes -- not a JSON library,
// the two message shapes a guest that runs one command and reports back
// needs (see the wire-protocol section below). Like `examples/serve/`
// and `examples/fetch/`, this is `extern fn` against libc through
// `Ffi("libc")` -- `AF_VSOCK` is a Linux-specific address family with no
// `Net` builtin behind it, and `docs/reach.md`'s whole point is that it
// does not need one: what decides whether a program is writable here is
// not a feature list, it is whether the authority it needs has a name,
// and libc already has one.
//
// `struct sockaddr_vm` (Linux's `<linux/vm_sockets.h>`) is 16 bytes:
// `svm_family` (`u16`) at 0, `svm_reserved1` (`u16`, must be 0) at 2,
// `svm_port` (`u32`) at 4, `svm_cid` (`u32`) at 8, then 4 zero bytes --
// checked directly against the real header (`offsetof`, a small C
// probe) rather than assumed. Unlike `struct sockaddr_in`'s port,
// `svm_port`/`svm_cid` are **host** byte order, not network byte order,
// so there is no endian flip to get right on these little-endian targets
// -- one thing this layout does not share with `examples/serve/`'s.
//
// `socket`/`connect`/`close` are `c_int`, not `int` (`docs/reach.md`
// §3.4): this file's own first draft used plain `int` and reported
// "connected" for a `connect` to a nonsense CID -- a real bug in how
// `extern fn` crossed a foreign return, found here and fixed at its
// root rather than worked around with a different check in this file.

import std.bytes;
import std.buffer;
import std.io;

extern fn socket[&f](ffi: &f Ffi("libc"), domain: int, kind: int, proto: int)
    -> [ffi("libc")] c_int;

extern fn connect[&f, &a](ffi: &f Ffi("libc"), fd: int, addr: &a [byte])
    -> [ffi("libc")] c_int;

extern fn close[&f](ffi: &f Ffi("libc"), fd: int) -> [ffi("libc")] c_int;

// `read`/`write` stay plain `int`: their real return is `ssize_t`,
// genuinely 64 bits here -- the same declaration `examples/fetch/`'s own
// header comment explains.
extern fn read[&f, &b](ffi: &f Ffi("libc"), fd: int, buf: &!b [byte]) -> [ffi("libc")] int;

extern fn write[&f, &b](ffi: &f Ffi("libc"), fd: int, buf: &b [byte]) -> [ffi("libc")] int;

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

// ---------------------------------------------------------------------
// The wire protocol -- `lex-os-proto`'s own `msg.rs`/`transport.rs`:
// one JSON object per line, newline-delimited, no length prefix
// (`StreamGuestTransport`'s `read_line`/`write_all` on the real vsock
// stream). What follows is not a JSON library -- it is exactly the two
// message shapes a guest that runs one command and reports back needs,
// the same "build what a program asks for" rule `AGENTS.md` §7 states
// for everything else in this repository. A `goal` containing a literal
// `"` or reordering `AgentViewMsg`'s own fields would defeat the decoder
// below; real `serde_json` output never reorders a struct's fields, and
// this file's own encoder escapes the one case that matters for command
// output (`"`, `\`, and the three common control bytes).
// ---------------------------------------------------------------------

fn append_json_escaped[&h, &r](heap: &!h Heap, b: buffer.Buffer, text: &r [byte]) -> [heap] buffer.Buffer {
    var buf = b;
    var i = 0;
    let n = len(text);
    while i < n {
        let c = int_of(text[i]);
        if c == 34 {
            buf = buffer.append(heap, buf, "\\\"");
        } else if c == 92 {
            buf = buffer.append(heap, buf, "\\\\");
        } else if c == 10 {
            buf = buffer.append(heap, buf, "\\n");
        } else if c == 13 {
            buf = buffer.append(heap, buf, "\\r");
        } else if c == 9 {
            buf = buffer.append(heap, buf, "\\t");
        } else {
            buf = buffer.push(heap, buf, text[i]);
        }
        i = i + 1;
    }
    return buf;
}

// `AgentActionMsg::Done` -- checked against real `serde_json` output:
// `{"action":"done"}`.
fn encode_done[&h](heap: &!h Heap, b: buffer.Buffer) -> [heap] buffer.Buffer {
    return buffer.append(heap, b, "{\"action\":\"done\"}");
}

// `AgentActionMsg::ExecResult` -- checked against real `serde_json`
// output on both a normal exit and a timed-out one with no exit code.
fn encode_exec_result[&h, &out, &err](
    heap: &!h Heap,
    b: buffer.Buffer,
    has_exit_code: bool,
    exit_code: int,
    out: &out [byte],
    err: &err [byte],
    timed_out: bool,
) -> [heap] buffer.Buffer {
    var buf = buffer.append(heap, b, "{\"action\":\"exec_result\",\"exit_code\":");
    if has_exit_code {
        buf = buffer.push_nat(heap, buf, exit_code);
    } else {
        buf = buffer.append(heap, buf, "null");
    }
    buf = buffer.append(heap, buf, ",\"stdout\":\"");
    buf = append_json_escaped(heap, buf, out);
    buf = buffer.append(heap, buf, "\",\"stderr\":\"");
    buf = append_json_escaped(heap, buf, err);
    buf = buffer.append(heap, buf, "\",\"timed_out\":");
    if timed_out {
        buf = buffer.append(heap, buf, "true");
    } else {
        buf = buffer.append(heap, buf, "false");
    }
    return buffer.append(heap, buf, "}");
}

// The index just past the first occurrence of `needle` in `hay` at or
// after `start`, or `-1`. `needle` is always a literal this file wrote,
// so a naive scan (no Boyer-Moore, no skip table) is the whole search
// this decoder ever needs.
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

// The end of the quoted string starting at `start` (the index right
// after its opening `"`) -- the first unescaped `"`. Goals and outcomes
// in this repository's own examples never contain a backslash, so
// skipping exactly one character after a `\` is enough to step over an
// escape without decoding it.
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

// The index right after `{"goal":"` in `line`, i.e. where the goal
// text itself starts -- or `-1` if `line` does not open that way.
fn goal_start_of[&line](line: &line [byte]) -> [] int {
    return find_after(line, "{\"goal\":\"", 0);
}

// The index of the `"` that closes the goal text starting at `start`,
// so a caller slices the goal out as `line[start..goal_end_of(line,
// start)]` with no further allocation -- or `-1` if it never closes.
fn goal_end_of[&line](line: &line [byte], start: int) -> [] int {
    return end_of_quoted(line, start);
}

// `AgentViewMsg`'s `step`, the field right after `completed`'s own
// close in real `lex-os-proto` JSON but read starting from wherever the
// caller's own scan of `goal` already reached, since `"step":` never
// repeats. `-1` on a decode failure: no `"step":` key found, or no
// digit after it.
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

// Read one newline-delimited JSON line from `fd` into `into` (a growable
// `Buffer`, since a real `AgentViewMsg` line's length is not known in
// advance) -- `StreamGuestTransport::recv_view`'s own shape, minus the
// deserialisation this file's own decoder does instead of `serde_json`.
// Returns the number of bytes in the line (the trailing `\n` dropped),
// or a negative number on a closed connection or a line that never fits
// the read loop's own bound (4096 chunks, sixteen of them: a real
// `AgentViewMsg` is a few hundred bytes at most).
fn recv_line[&h, &f](
    heap: &!h Heap,
    libc: &f Ffi("libc"),
    fd: int,
    into: buffer.Buffer,
) -> [heap, ffi("libc")] (buffer.Buffer, int) {
    region scratch {
        let chunk = alloc_slice[scratch](4096, byte_of(0));
        var buf = into;
        var rounds = 0;
        while rounds < 16 {
            let got = read(libc, fd, chunk);
            if got <= 0 {
                return (buf, 0 - 1);
            }
            var i = 0;
            while i < got {
                if int_of(chunk[i]) == 10 {
                    var n = 0;
                    borrow buf as &bb in {
                        n = buffer.size(bb);
                    }
                    return (buf, n);
                }
                buf = buffer.push(heap, buf, chunk[i]);
                i = i + 1;
            }
            rounds = rounds + 1;
        }
        return (buf, 0 - 1);
    }
}

// Write all of `line` plus a trailing newline -- `send_action`'s own
// shape in `StreamGuestTransport`.
fn send_line[&f, &b](libc: &f Ffi("libc"), fd: int, line: &b [byte]) -> [ffi("libc")] bool {
    var sent = 0;
    let n = len(line);
    while sent < n {
        let got = write(libc, fd, line[sent..n]);
        if got <= 0 {
            return false;
        }
        sent = sent + got;
    }
    let nl = "\n";
    return write(libc, fd, nl) == 1;
}

// After a successful `dial()`: read one `AgentViewMsg` line, print the
// goal and step it carries, then answer with a `Done` action -- the
// smallest exchange that exercises both directions of the real
// `lex-os-proto` wire protocol rather than just the connect this file
// used to stop at. Returns the exit status `main` should report.
//
// This function reads/writes an `fd` and never touches `AF_VSOCK`
// itself (that is all in `dial`), so it was verified end to end on a
// real `AF_UNIX` `socketpair` standing in for the byte-stream half of
// the connection -- a scripted host wrote a real, byte-exact
// `AgentViewMsg` line and read back a byte-exact `Done` action -- rather
// than trusting the JSON layer's own unit checks to compose correctly.
// This sandbox has no `vhost_vsock`, so an actual `AF_VSOCK` round trip
// against `lex-os-guest`/its supervisor stays untested here; that gap
// is real and unclosed by this change.
fn converse[&h, &f, &i](
    heap: &!h Heap,
    libc: &f Ffi("libc"),
    io: &!i Io,
    fd: int,
) -> [heap, ffi("libc"), io_write, err_write] int {
    var status = 0;
    let (line, n) = recv_line(heap, libc, fd, buffer.empty(heap, 4096));
    if n < 0 {
        io.error_all(io, "vsock: connection closed before a full line arrived\n");
        status = 4;
    } else {
        borrow line as &l in {
            let view = buffer.bytes(l);
            let goal_start = goal_start_of(view);
            if goal_start < 0 {
                io.error_all(io, "vsock: could not find \"goal\" in the view\n");
                status = 4;
            } else {
                let goal_end = goal_end_of(view, goal_start);
                let step = step_of(view, goal_start);
                if goal_end < 0 || step < 0 {
                    io.error_all(io, "vsock: malformed view line\n");
                    status = 4;
                } else {
                    io.write_all(io, "goal: ");
                    io.write_all(io, view[goal_start..goal_end]);
                    io.write_all(io, "\n");
                    var out = buffer.empty(heap, 64);
                    out = buffer.append(heap, out, "step: ");
                    out = buffer.push_nat(heap, out, step);
                    out = buffer.push(heap, out, byte_of(10));
                    borrow out as &o in {
                        io.write_all(io, buffer.bytes(o));
                    }
                    buffer.drop(heap, out);
                }
            }
        }
        var reply = buffer.empty(heap, 32);
        reply = encode_done(heap, reply);
        var sent = false;
        borrow reply as &r in {
            sent = send_line(libc, fd, buffer.bytes(r));
        }
        buffer.drop(heap, reply);
        if !sent {
            io.error_all(io, "vsock: could not send the done action\n");
            status = 5;
        }
    }
    buffer.drop(heap, line);
    return status;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(fs);

    let libc = narrow(ffi, "libc");
    var status = 2;
    borrow mut heap as &!h in {
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
                                    status = converse(h, f, i, fd);
                                    close(f, fd);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    release(heap);
    release(libc);
    release(args);
    release(io);
    return status;
}
