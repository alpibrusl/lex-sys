// cut.ls — GNU `cut -d<delim> -f<list>`, and the third port here.
//
// Chosen as a probe rather than a demonstration: `docs/utf8.md` §1 says
// the rest of a string library is "code, not design", and the way to
// find out *which* code is to write a program that needs it and see
// what it hand-rolls. `vec.set` and `vec.swap` arrived that way
// (`porting.md` §9.3), and so did the four functions `sort.ls` asked
// for.
//
// What it supports is `-d` and `-f`: a one-byte delimiter and a list of
// fields, `1,3-5,7-` in cut's own syntax. Lines without the delimiter
// pass through whole, which is what GNU does without `-s`.
//
//     lex-sys run examples/cut/cut.ls --std -- -d, -f2,4 < data.csv
//
// The fields themselves are `bytes.field`, which this program is the
// reason for -- and so is `buffer.clear`, which it asked for by not
// being able to reuse a line buffer without one (`line-reading.md` §4).
//
// It also asked for something it did **not** get. `ROADMAP.md` wanted a
// `read_line` in `std` because two programs supposedly hand-rolled this
// loop; one of them keeps no buffer at all, so this is the only program
// that reads a line at a time and the bar is two. §1 there is the
// count, taken by reading them.

import std.io;
import std.bytes;
import std.buffer;

// ---------------------------------------------------------- the list -----

// Which fields were asked for is a bitmap over 1..=`max_field`, plus an
// `open_at` for a range like `7-` that has no upper bound. Two values
// rather than a struct, because a struct cannot hold a `[byte]` -- it
// has no size of its own and a field would have to be a slice, which
// would need the region in the type. That is `boxed-slices.md`'s line
// in practice and it is the right refusal: the bitmap lives in the
// arena and the pair travels as arguments.
//
// A bitmap rather than a list because membership is the only question
// asked, and it is asked once per field per line.
fn max_field() -> [] int {
    return 1024;
}

// Parse cut's `-f` syntax. Answers `-1` in `from_open` on a malformed
// list, which `main` turns into the exit status GNU uses.
//
// That sentence was **false** when it was written: GNU exits 1 and this
// exited 2, on the same input. The conformance test ran that input and
// missed it, because it asserted the status this program *had* while
// comparing the eight valid specs against the real `/usr/bin/cut` --
// and it did that because a failing run produced no output to compare.
// `docs/standard-error.md` §1.3: a stream nobody has is a comparison
// nobody makes.
fn parse_list[&r, &o](spec: &r [byte], bits: &!o [byte]) -> [] int {
    var open_at = 0;
    var at = 0;
    var bad = false;
    while at < len(spec) && !bad {
        var lo = 0;
        var saw = false;
        while at < len(spec) && bytes.is_digit(int_of(spec[at])) {
            lo = lo * 10 + bytes.digit_of(int_of(spec[at]));
            at = at + 1;
            saw = true;
        }
        if !saw { bad = true; }
        else {
            var hi = lo;
            if at < len(spec) && int_of(spec[at]) == '-' {
                at = at + 1;
                hi = 0;
                var saw_hi = false;
                while at < len(spec) && bytes.is_digit(int_of(spec[at])) {
                    hi = hi * 10 + bytes.digit_of(int_of(spec[at]));
                    at = at + 1;
                    saw_hi = true;
                }
                // `n-` with nothing after it: everything from n on.
                if !saw_hi {
                    hi = 0;
                    if open_at == 0 || lo < open_at { open_at = lo; }
                }
            }
            if lo < 1 { bad = true; }
            var n = lo;
            while n <= hi && n <= max_field() {
                bits[n] = byte_of(1);
                n = n + 1;
            }
            if at < len(spec) {
                if int_of(spec[at]) == ',' { at = at + 1; }
                else { bad = true; }
            }
        }
    }
    if bad { return 0 - 1; }
    return open_at;
}

fn is_wanted[&o](bits: &o [byte], from_open: int, n: int) -> [] bool {
    if from_open > 0 && n >= from_open { return true; }
    if n > max_field() { return false; }
    return int_of(bits[n]) == 1;
}

// ---------------------------------------------------------- the work -----

// One line, cut. Answers how many fields it wrote.
fn cut_line[&i, &l, &o](io: &!i Io, line: &l [byte], delim: int,
    bits: &o [byte], from_open: int) -> [io_write] int {
    let fields = bytes.count_byte(line, delim) + 1;
    // GNU passes a line with no delimiter through whole (no `-s`).
    if fields == 1 {
        io.write_all(io, line);
        io.newline(io);
        return 1;
    }
    var written = 0;
    var n = 1;
    while n <= fields {
        if is_wanted(bits, from_open, n) {
            if written > 0 { putchar(io, delim); }
            io.write_all(io, bytes.field(line, delim, n));
            written = written + 1;
        }
        n = n + 1;
    }
    io.newline(io);
    return written;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(ffi); release(fs);

    var delim = 9;
    var spec = "1";
    var usage = false;
    borrow args as &g in {
        var n = 1;
        while n < arg_count(g) {
            let a = arg(g, n);
            if bytes.starts_with(a, "-d") && len(a) == 3 {
                delim = int_of(a[2]);
            } else {
                if bytes.starts_with(a, "-f") && len(a) > 2 {
                    spec = a[2..len(a)];
                } else {
                    usage = true;
                }
            }
            n = n + 1;
        }
    }
    release(args);

    var status = 0;
    if usage {
        status = 1;
        borrow mut io as &!i in {
            io.error_all(i, "cut: usage: cut -d<c> -f<list>\n");
        }
    }

    if status == 0 {
        region a {
            // `alloc_slice` hands back a slice, which *is* a reference
            // already -- so this is passed directly rather than through
            // a `borrow mut`, which would be a reference to a reference.
            var bits = alloc_slice[a](max_field() + 1, byte_of(0));
            let from_open = parse_list(spec, bits);
            if from_open < 0 {
                status = 1;
                // GNU's own wording, on GNU's own stream. Four calls
                // rather than one because there is no formatting into a
                // buffer here and the spec is a slice of the argument
                // (`docs/standard-error.md` §8).
                borrow mut io as &!i in {
                    io.error_all(i, "cut: invalid field value '");
                    io.error_all(i, spec);
                    io.error_all(i, "'\n");
                }
            }
            else {
                // One line at a time: `getchar` is the only input
                // primitive there is (`standard-input.md`), and a line
                // is the unit `cut` works in.
                //
                // **On the heap, because the arena version was wrong.**
                // A region is one 64 KiB chunk (`heap.md`), so the line
                // buffer was 60 000 bytes — what fits beside the bitmap
                // — and a longer line was truncated *silently*. That is
                // not merely a limit: a truncated line loses its
                // delimiters, so this program switched to the
                // no-delimiter rule and printed 60 KB of the wrong
                // field with exit 0. `docs/line-reading.md` §2 has the
                // table. `std.buffer` grows, the way `examples/sort/`
                // does, and there is no limit left to get wrong.
                borrow mut heap as &!h in {
                    var line = buffer.empty(h, 4096);
                    borrow mut io as &!i in {
                        var c = getchar(i);
                        while c >= 0 {
                            if c == '\n' {
                                borrow line as &l in {
                                    cut_line(i, buffer.bytes(l), delim, bits, from_open);
                                }
                                // `clear` keeps the allocation, so the
                                // next line writes into room this one
                                // already grew (§4).
                                borrow mut line as &!l in { buffer.clear(l); }
                            } else {
                                line = buffer.push(h, line, byte_of(c));
                            }
                            c = getchar(i);
                        }
                        // A final line with no newline still counts, and
                        // gets one on the way out -- as GNU does.
                        var last = 0;
                        borrow line as &l in { last = buffer.size(l); }
                        if last > 0 {
                            borrow line as &l in {
                                cut_line(i, buffer.bytes(l), delim, bits, from_open);
                            }
                        }
                    }
                    buffer.drop(h, line);
                }
            }
        }
    }

    release(heap);
    release(io);
    return status;
}
