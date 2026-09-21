// `lines` -- a small log tool, and M3's acceptance criterion.
//
// It writes a log, reads it back off disk, counts and filters it, writes a
// report, and reads the report back to print it. Four file operations, one
// capability, and a row on every frame that says which part of the
// filesystem it can touch.
//
// What is worth reading here is not the file handling, which is two calls
// (`docs/filesystem.md` §3). It is that the *authority* to do it is an
// ordinary linear value: `main` is handed one `World`, narrows it once to
// `/tmp`, lends it down the call chain, and destroys it. Nothing in this
// program can touch a path outside `/tmp`, and that is not a convention --
// `report` could not be *written* to do it, because its row would have to
// say so and its capability could not be widened to match.
//
// Everything else is M3's slices: a line is a pair of offsets into a byte
// buffer, because a slice of a slice is not a thing this language has yet
// and offsets are what a tool would use anyway.
//~ STDOUT lines 6
//~ STDOUT errors 2
//~ STDOUT longest 15
//~ EXIT 0

// ---------------------------------------------------------------------
// Console
// ---------------------------------------------------------------------

fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io_write] int {
    var n = 0;
    while n < len(s) {
        putchar(io, int_of(s[n]));
        n = n + 1;
    }
    return len(s);
}

// ---------------------------------------------------------------------
// Building bytes in a buffer
// ---------------------------------------------------------------------

// Copy `s` into `into` starting at `at`, and answer where the next byte
// goes. The buffer is unique, so this is the only reference to it that
// exists while the copy happens -- there is no aliasing question to ask.
fn append[&b, &r](into: &!b [byte], at: int, s: &r [byte]) -> [] int {
    var n = 0;
    while n < len(s) {
        into[at + n] = s[n];
        n = n + 1;
    }
    return at + len(s);
}

// The same, for a number. Recursive because the digits come out backwards
// otherwise, and a recursion two levels deep is cheaper to read than a
// reversal loop.
fn append_nat[&b](into: &!b [byte], at: int, n: int) -> [] int {
    var next = at;
    if n >= 10 {
        next = append_nat(into, at, n / 10);
    }
    into[next] = byte_of(48 + n % 10);
    return next + 1;
}

// ---------------------------------------------------------------------
// Reading the log
// ---------------------------------------------------------------------

// Does `hay` carry `needle` at offset `at`?
//
// `==` on `byte` compares storage, which is allowed; `+` on `byte` is
// arithmetic, which is not (`docs/strings.md` §2). A scanner only ever
// needs the first.
fn matches_at[&h, &n](hay: &h [byte], at: int, needle: &n [byte]) -> [] bool {
    if at + len(needle) > len(hay) {
        return false;
    }
    var i = 0;
    while i < len(needle) {
        if hay[at + i] != needle[i] {
            return false;
        }
        i = i + 1;
    }
    return true;
}

struct Counts {
    lines: int,
    errors: int,
    longest: int,
}

// One pass over the bytes. A line ends at a newline or at the end of the
// buffer, and `start` is where the current one began.
fn tally[&b](text: &b [byte], length: int) -> [] Counts {
    var lines = 0;
    var errors = 0;
    var longest = 0;
    var start = 0;
    var i = 0;
    while i < length {
        if text[i] == byte_of(10) {
            let width = i - start;
            lines = lines + 1;
            if width > longest {
                longest = width;
            }
            if matches_at(text, start, "ERROR") {
                errors = errors + 1;
            }
            start = i + 1;
        }
        i = i + 1;
    }
    return Counts { lines: lines, errors: errors, longest: longest };
}

// ---------------------------------------------------------------------
// The tool
// ---------------------------------------------------------------------

// The row is the documentation. A caller that wanted to know whether this
// function could read `/etc` would not have to look inside it -- and the
// honest answer here is *yes*, which is the point of §1 below.
fn report[&f, &p, &i](
    fs: &f Fs(""),
    path: &p [byte],
    io: &!i Io,
) -> [fs_read(""), fs_write(""), io_write] int {
    var status = 1;
    region a {
        let text = alloc_slice[a](512, byte_of(0));
        let read = fs_read(fs, path, text);
        if read < 0 {
            // A missing file is an ordinary outcome and comes back as -1,
            // not a trap (`docs/filesystem.md` §3). A path outside what the
            // capability granted *would* trap -- but this one granted
            // everything, so nothing here is outside it.
            write_all(io, "cannot read that file\n");
            return 1;
        }

        let counts = tally(text, read);

        // The report is built in the same arena and written out in one
        // call. Both buffers die when the region does -- one `free`, for
        // everything this function allocated (§6).
        let out = alloc_slice[a](64, byte_of(0));
        var at = append(out, 0, "lines ");
        at = append_nat(out, at, counts.lines);
        at = append(out, at, "\nerrors ");
        at = append_nat(out, at, counts.errors);
        at = append(out, at, "\nlongest ");
        at = append_nat(out, at, counts.longest);
        at = append(out, at, "\n");

        // Only the part that was filled. `out` is 64 bytes long and the
        // report is shorter, so the length is the thing that has to be
        // right here -- and it is an ordinary `int` the program computed,
        // not something the buffer knows.
        let short = alloc_slice[a](at, byte_of(0));
        var n = 0;
        while n < at {
            short[n] = out[n];
            n = n + 1;
        }
        fs_write(fs, "/tmp/lex-sys-lines.report", short);

        // Read it back rather than printing what is already in hand: the
        // point of the exercise is the round trip, and a report nobody
        // re-reads is a report that could have been wrong on disk.
        let back = alloc_slice[a](64, byte_of(0));
        let size = fs_read(fs, "/tmp/lex-sys-lines.report", back);
        var k = 0;
        while k < size {
            putchar(io, int_of(back[k]));
            k = k + 1;
        }
        if size == at {
            status = 0;
        }
    }
    return status;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program allocates nothing on the heap, so that authority ends here.
    release(heap);
    // File operations reach libc from the backend rather than through an
    // `extern fn`, so a program that touches files needs no FFI capability
    // at all -- which is the whole reason `Fs` means anything (§2).
    release(ffi);

    // §1: `fs` is passed on **unnarrowed**, and that is a decision rather
    // than an oversight.
    //
    // The path comes from the command line, and the prefix a capability is
    // narrowed to is a *literal* checked where it is written
    // (`linearity-and-effects.md` §7.4). So a tool that reads a path the
    // user chose cannot narrow to it: there is no literal to narrow to.
    // Narrowing to `/tmp` would make `lines /home/me/x.log` trap, which is
    // not a safer tool, it is a broken one.
    //
    // What the design still buys, with the capability at its widest: the
    // row on `report` says `fs_read("")` and `fs_write("")`, so a reader
    // knows this program reads and writes arbitrary paths without opening
    // the body. A tool that *did* know its directory would narrow, and its
    // row would say so instead. The type tells the truth either way.
    var status = 1;
    borrow fs as &f in {
        borrow args as &g in {
            borrow mut io as &!i in {
                if arg_count(g) > 1 {
                    // Given a path, read that file.
                    status = report(f, arg(g, 1), i);
                } else {
                    // Given nothing, lay down a sample and read that --
                    // which is how a tool with no input behaves anyway,
                    // and is what lets a harness that passes no arguments
                    // still run this file.
                    fs_write(
                        f,
                        "/tmp/lex-sys-lines.log",
                        "INFO  boot\nERROR disk full\nINFO  retry\nERROR disk full\nWARN  slow\nINFO  done\n",
                    );
                    status = report(f, "/tmp/lex-sys-lines.log", i);
                }
            }
        }
    }
    release(fs);
    release(args);
    release(io);
    return status;
}
