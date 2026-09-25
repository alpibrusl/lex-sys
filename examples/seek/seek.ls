// `seek` -- a literal-text search over named files, shaped for an agent
// calling it rather than for a person typing `grep` at a terminal.
//
// `docs/agent-tools.md` is the design; this is not a `grep` port the way
// `examples/base64/`, `examples/cut/` and `examples/sort/` are GNU ports
// -- there is no upstream this is checked byte-for-byte against, because
// the point is not compatibility. It is: does the same effect-typed,
// no-UB, no-silent-truncation discipline those three ports already have
// produce a *better* tool for the caller this repository has not built
// for yet, an agent rather than a shell?
//
//     lex-sys run examples/seek/seek.ls --std -- [-n] [-c] [-m <count>] <pattern> <file>...
//
// `-n` prefixes a match with its 1-based line number; `-c` prints a
// count instead of the matching lines; `-m <count>` stops after that
// many matches *in total*, not per file the way GNU's `-m` does
// (`docs/agent-tools.md` §3.2 says why: this tool has no per-file
// concept until a file is actually open, and threading one through
// costs a second counter for a distinction an agent capping its own
// context rarely needs). Reading no files at all reads standard input,
// matching `examples/sort/`'s own convention.
//
// Exit status is GNU grep's own vocabulary, not invented here: `0` for
// at least one match, `1` for none, `2` for a usage or read error -- a
// caller that already knows `grep` already knows this program.

import std.buffer;
import std.io;
import std.bytes;
import std.flags;

// ---------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------

// Standard input, one byte at a time, appended to a buffer that grows --
// `examples/sort/`'s own `read_stdin`, unchanged.
fn read_stdin[&h, &i](heap: &!h Heap, io: &!i Io, text: buffer.Buffer)
    -> [heap, io_read] buffer.Buffer {
    var out = text;
    var c = getchar(io);
    while c >= 0 {
        out = buffer.push(heap, out, byte_of(c));
        c = getchar(io);
    }
    return out;
}

// One named file, read whole through a handle -- `examples/sort/`'s own
// `read_file`, the fix `docs/file-handles.md` §1 measured already
// applied: one pass, no doubling retry, no ceiling. `-1` on a read
// failure; the buffer comes back either way, because an error is not a
// reason to leak.
fn read_file[&h, &f, &p](heap: &!h Heap, fs: &f Fs(""), path: &p [byte],
    text: buffer.Buffer) -> [heap, fs_read(""), file_read] (buffer.Buffer, int) {
    var out = text;
    var total = 0;
    var trouble = 0;

    match open_read(fs, path) {
        Opened::Failed(reason) => {
            trouble = reason;
            if trouble == 0 {
                trouble = 1;
            }
        }
        Opened::Ok(opened) => {
            var file = opened;
            var chunk = buffer.empty(heap, 65536);
            borrow mut file as &!handle in {
                var going = true;
                while going {
                    var got = 0;
                    borrow mut chunk as &!c in {
                        match file_read(handle, buffer.room(c)) {
                            Read::Got(n) => {
                                got = n;
                                buffer.filled(c, n);
                            }
                            Read::End => {
                                going = false;
                            }
                            Read::Failed(reason) => {
                                going = false;
                                trouble = reason;
                                if trouble == 0 {
                                    trouble = 1;
                                }
                            }
                        }
                    }
                    if got > 0 {
                        borrow chunk as &c in {
                            out = buffer.append(heap, out, buffer.bytes(c));
                        }
                        total = total + got;
                        borrow mut chunk as &!c in {
                            buffer.clear(c);
                        }
                    }
                }
            }
            file_close(file);
            buffer.drop(heap, chunk);
        }
    }

    if trouble != 0 {
        return (out, 0 - 1);
    }
    return (out, total);
}

// ---------------------------------------------------------------------
// Searching
// ---------------------------------------------------------------------

// A non-negative integer from `-m`'s value, or `-1` for anything that is
// not one -- an empty value, a sign, a non-digit. `-1` is not a count
// `-m` could ever mean, the same "unparseable reads as refused, not as
// absent" choice `lex-sys-codegen`'s own `port_bound_of` makes for a
// malformed `Net` bound (`docs/listen.md` §6.2).
fn parse_nat[&s](text: &s [byte]) -> [] int {
    if len(text) == 0 {
        return 0 - 1;
    }
    var n = 0;
    var i = 0;
    while i < len(text) {
        let c = int_of(text[i]);
        if !bytes.is_digit(c) {
            return 0 - 1;
        }
        n = n * 10 + bytes.digit_of(c);
        i = i + 1;
    }
    return n;
}

// One file's worth of lines, tested against `pattern` and printed (or
// counted) as they match. `total` is the running match count across
// every file `main` has searched so far, threaded through by value the
// way `examples/sort/`'s `(grown, got)` already is; `max_count < 0`
// means unbounded. Returns `(new total, this file's own count)`.
//
// A byte-for-byte search over whatever bytes a file holds -- no encoding
// is assumed and none is checked, so a file that is not UTF-8, or not
// text at all, is searched exactly the same way and cannot make this
// trap or misbehave (`docs/defined-behaviour.md` §2.1's guarantee,
// exercised here rather than only claimed).
fn search_lines[&i, &t, &p, &f](io: &!i Io, text: &t [byte], pattern: &p [byte],
    name: &f [byte], show_name: bool, show_lines: bool, count_only: bool,
    max_count: int, total: int) -> [io_write] (int, int) {
    var running = total;
    var found = 0;
    var line_no = 0;
    var at = 0;
    while at < len(text) && (max_count < 0 || running < max_count) {
        var end = at;
        while end < len(text) && int_of(text[end]) != '\n' {
            end = end + 1;
        }
        line_no = line_no + 1;
        let line = text[at..end];
        if bytes.find(line, pattern) >= 0 {
            found = found + 1;
            running = running + 1;
            if !count_only {
                if show_name {
                    io.write_all(io, name);
                    putchar(io, ':');
                }
                if show_lines {
                    io.print_nat(io, line_no);
                    putchar(io, ':');
                }
                io.write_all(io, line);
                putchar(io, 10);
            }
        }
        at = end + 1;
    }
    if count_only {
        if show_name {
            io.write_all(io, name);
            putchar(io, ':');
        }
        io.print_nat(io, found);
        putchar(io, 10);
    }
    return (running, found);
}

// ---------------------------------------------------------------------

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // A search calls no C and narrows the filesystem to nothing beyond
    // what `Fs("")` already is -- `docs/agent-tools.md` §2 is the report
    // this buys: `lex-sys authority` on this program names exactly
    // `args`, `fs_read`, `heap`, `io_write` (`io_read` too, only when it
    // falls back to standard input) and nothing wider, checked by
    // `agent_tools.rs`'s own `seek_reports_a_bounded_authority`.
    release(ffi);

    var show_lines = false;
    var count_only = false;
    var max_count = 0 - 1;
    var usage = false;
    var pattern_set = false;
    var pattern: &static [byte] = "";
    var file_count = 0;
    var status = 0;
    var error_seen = false;
    var total = 0;

    borrow mut heap as &!h in {
        borrow args as &g in {
            var c = flags.start();
            var going = true;
            while going {
                let (next, step) = flags.step(g, c);
                c = next;
                match step {
                    flags.Arg::Short(letter) => {
                        if letter == 'n' {
                            show_lines = true;
                        } else if letter == 'c' {
                            count_only = true;
                        } else if letter == 'm' {
                            let (after, given) = flags.value(g, c);
                            c = after;
                            max_count = parse_nat(given);
                            if max_count < 0 {
                                usage = true;
                            }
                        } else {
                            usage = true;
                        }
                    }
                    flags.Arg::Long(name) => {
                        if flags.named(name, "line-number") {
                            show_lines = true;
                        } else if flags.named(name, "count") {
                            count_only = true;
                        } else if flags.named(name, "max-count") {
                            let (after, given) = flags.value(g, c);
                            c = after;
                            max_count = parse_nat(given);
                            if max_count < 0 {
                                usage = true;
                            }
                        } else {
                            usage = true;
                        }
                    }
                    flags.Arg::Operand(text) => {
                        if !pattern_set {
                            pattern = text;
                            pattern_set = true;
                        } else if usage {
                            // Already refused; do not spend a file read
                            // on an invocation that is going to exit 2
                            // anyway.
                        } else if max_count >= 0 && total >= max_count {
                            // The budget is already spent -- an agent
                            // that asked for at most five matches should
                            // not pay for a sixth file's read.
                        } else {
                            file_count = file_count + 1;
                            borrow fs as &f in {
                                var scratch = buffer.empty(h, 65536);
                                let (grown, got) = read_file(h, f, text, scratch);
                                scratch = grown;
                                if got == 0 - 1 {
                                    error_seen = true;
                                    borrow mut io as &!e in {
                                        io.error_all(e, "seek: cannot read: ");
                                        io.error_all(e, text);
                                        io.error_all(e, "\n");
                                    }
                                } else {
                                    borrow mut io as &!o in {
                                        borrow scratch as &s in {
                                            let (grown_total, found_here) = search_lines(o,
                                                buffer.bytes(s), pattern, text, true,
                                                show_lines, count_only, max_count, total);
                                            total = grown_total;
                                        }
                                    }
                                }
                                buffer.drop(h, scratch);
                            }
                        }
                    }
                    flags.Arg::Done => {
                        going = false;
                    }
                }
            }
        }

        if !pattern_set {
            usage = true;
        }

        if usage {
            status = 2;
            borrow mut io as &!e in {
                io.error_all(e, "seek: usage: seek [-n] [-c] [-m <count>] <pattern> <file>...\n");
            }
        } else if file_count == 0 {
            var text = buffer.empty(h, 65536);
            borrow mut io as &!i in {
                text = read_stdin(h, i, text);
            }
            borrow mut io as &!o in {
                borrow text as &t in {
                    let (grown_total, found_here) =
                        search_lines(o, buffer.bytes(t), pattern, "-", false, show_lines,
                            count_only, max_count, total);
                    total = grown_total;
                }
            }
            buffer.drop(h, text);
        }
    }

    if !usage {
        if error_seen {
            status = 2;
        } else if total > 0 {
            status = 0;
        } else {
            status = 1;
        }
    }

    release(args);
    release(fs);
    release(io);
    release(heap);
    return status;
}
