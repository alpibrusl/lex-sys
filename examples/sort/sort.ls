// `sort` -- the second port, and the one with resources in it.
//
// `docs/porting.md` §6 said what `base64` had not tested: linearity at
// scale, effect rows at depth, `borrow mut`'s strictness, and the heap.
// This program was chosen to need all four. It is `LC_ALL=C sort` with no
// flags: read the files named on the command line (or standard input when
// none are), sort the lines by byte order, write them out.
//
// The conformance suite runs it against GNU `sort` with `LC_ALL=C` and
// compares the bytes, so "the same" is not a claim made here.
//
// **Five owned resources**, and the heap holds all of them: the text, two
// parallel runs describing where each line is, and the permutation being
// sorted with its scratch. Every one is created in `main`, lent down, and
// destroyed there -- and the row on every function below says which of
// them it touches.
//
// What it needed that did not exist: `vec.set` and `vec.swap`. A vector
// could be read and appended to but never written, which nothing had
// noticed because nothing had tried to sort one (§9).

import std.buffer;
import std.vec;
import std.io;
import std.bytes;

// ---------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------

// Standard input, one byte at a time, appended to a buffer that grows.
//
// `push` takes the buffer by value and hands it back, so the loop
// **moves** it round and round: `text = buffer.push(h, text, ...)`. That
// is linearity at its most ordinary, and it is the shape every function
// here has.
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

// One named file, appended.
//
// `fs_read` fills as much of the slice as the file has and returns the
// count (`docs/filesystem.md` §3). There are no handles and no way to ask
// how big a file is, so a file that exactly fills the scratch might have
// been truncated -- which is why this doubles and reads again rather than
// trusting the first answer. §9.1 is about what that costs.
//
// Answers -1 if the file could not be read and -2 if it is larger than
// this can grow to hold. Two codes rather than one because they are two
// different things to tell a user, and `docs/file-handles.md` §1.2 is
// what collapsing them cost: a file past the ceiling looked exactly
// like a typo in a filename.
//
// The buffer comes back either way: an error is not a reason to leak.
fn read_file[&h, &f, &p](heap: &!h Heap, fs: &f Fs(""), path: &p [byte],
    text: buffer.Buffer) -> [heap, fs_read("")] (buffer.Buffer, int) {
    var out = text;
    var capacity = 65536;
    var attempts = 0;
    // Fifteen attempts from 64 KiB reach a largest capacity of 1 GiB:
    // 65536 * 2^14. The read has to come back *strictly* shorter than
    // the capacity to be believed, so that is the largest file this can
    // hold -- and a sort that keeps the whole file in memory has worse
    // problems past a gigabyte.
    //
    // It was eight, which reached 8 MiB, under a comment claiming 16 --
    // it counted doublings where the loop counts attempts, so the one
    // place a reader would look for the limit said twice the real one
    // and `examples/sort/` quietly refused an 8 MiB file.
    // `docs/file-handles.md` §1.1 is the measurement.
    //
    // A ceiling at all, rather than growing until something gives:
    // `heap.md` says an allocation that fails **traps**, so without one
    // a file bigger than memory would abort instead of reporting an
    // error, and a sort should be able to say "too big" out loud.
    while attempts < 15 {
        var got = 0;
        var scratch = buffer.empty(heap, capacity);
        borrow mut scratch as &!s in {
            got = fs_read(fs, path, buffer.room(s));
            if got > 0 {
                buffer.filled(s, got);
            }
        }
        if got < 0 {
            buffer.drop(heap, scratch);
            return (out, 0 - 1);
        }
        if got < capacity {
            borrow scratch as &s in {
                out = buffer.append(heap, out, buffer.bytes(s));
            }
            buffer.drop(heap, scratch);
            return (out, got);
        }
        // It filled the slice exactly, so it may have been cut short.
        buffer.drop(heap, scratch);
        capacity = capacity * 2;
        attempts = attempts + 1;
    }
    return (out, 0 - 2);
}

// ---------------------------------------------------------------------
// Lines
// ---------------------------------------------------------------------

// Where each line starts and how long it is, not counting the newline.
//
// A trailing byte that is not a newline still ends a line, which is what
// `sort` does: a file without a final newline gets one on the way out.
fn find_lines[&h, &t](heap: &!h Heap, text: &t [byte], starts: vec.Vec[int],
    lengths: vec.Vec[int]) -> [heap] (vec.Vec[int], vec.Vec[int]) {
    var s = starts;
    var l = lengths;
    var at = 0;
    while at < len(text) {
        var end = at;
        while end < len(text) && int_of(text[end]) != 10 {
            end = end + 1;
        }
        s = vec.push(heap, s, at);
        l = vec.push(heap, l, end - at);
        at = end + 1;
    }
    return (s, l);
}

// Byte order, shorter first when one is a prefix of the other. That is
// `LC_ALL=C` -- and the reason the suite sets it, because a locale would
// compare these differently and this program does not have one.
//
// The rule is `std.bytes.compare` now; this is the call site that made
// it worth writing down. Two slices rather than the `(text, at, len)`
// triples this used to take, because `slicing.md` §1's subslice is
// exactly that triple with the arithmetic done once -- and a subslice
// cannot outlive `text`, which the triples could not promise.
fn before[&t](text: &t [byte], a_at: int, a_len: int, b_at: int, b_len: int)
    -> [] bool {
    return bytes.compare(text[a_at..a_at + a_len], text[b_at..b_at + b_len]) < 0;
}

// ---------------------------------------------------------------------
// The sort
// ---------------------------------------------------------------------

// Merge `[lo, mid)` and `[mid, hi)` of `order`, using `scratch` for the
// left half.
//
// Five references at once, which is the depth this program was chosen
// for: the text and the two line runs shared, the permutation and its
// scratch unique. The checker keeps them apart because they are different
// values -- `borrow mut` freezes what it borrows, and nothing here
// borrows the same thing twice.
fn merge[&t, &p, &q, &o, &s](text: &t [byte], starts: &p vec.Vec[int],
    lengths: &q vec.Vec[int], order: &!o vec.Vec[int],
    scratch: &!s vec.Vec[int], lo: int, mid: int, hi: int) -> [] int {
    var i = lo;
    while i < mid {
        vec.set(scratch, i, vec.get(order, i));
        i = i + 1;
    }

    var left = lo;
    var right = mid;
    var out = lo;
    while left < mid && right < hi {
        let a = vec.get(scratch, left);
        let b = vec.get(order, right);
        // `before(b, a)` rather than `!before(a, b)`, so equal lines keep
        // the order they arrived in. GNU's default sort is not stable and
        // this does not need to be, but a stable merge is not more code
        // and a reader should not have to wonder.
        if before(text, vec.get(starts, b), vec.get(lengths, b),
                  vec.get(starts, a), vec.get(lengths, a)) {
            vec.set(order, out, b);
            right = right + 1;
        } else {
            vec.set(order, out, a);
            left = left + 1;
        }
        out = out + 1;
    }
    while left < mid {
        vec.set(order, out, vec.get(scratch, left));
        left = left + 1;
        out = out + 1;
    }
    return out;
}

// Top-down merge sort over `[lo, hi)`.
fn msort[&t, &p, &q, &o, &s](text: &t [byte], starts: &p vec.Vec[int],
    lengths: &q vec.Vec[int], order: &!o vec.Vec[int],
    scratch: &!s vec.Vec[int], lo: int, hi: int) -> [] int {
    if hi - lo < 2 {
        return 0;
    }
    let mid = lo + (hi - lo) / 2;
    msort(text, starts, lengths, order, scratch, lo, mid);
    msort(text, starts, lengths, order, scratch, mid, hi);
    merge(text, starts, lengths, order, scratch, lo, mid, hi);
    return 0;
}

// ---------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------

fn write_lines[&i, &t, &p, &q, &o](io: &!i Io, text: &t [byte],
    starts: &p vec.Vec[int], lengths: &q vec.Vec[int],
    order: &o vec.Vec[int], count: int) -> [io_write] int {
    var n = 0;
    while n < count {
        let which = vec.get(order, n);
        let at = vec.get(starts, which);
        let length = vec.get(lengths, which);
        io.write_all(io, text[at..at + length]);
        putchar(io, 10);
        n = n + 1;
    }
    return count;
}

// ---------------------------------------------------------------------

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // A sort calls no C. Everything else it is handed, it uses.
    release(ffi);

    var status = 0;
    borrow mut heap as &!h in {
        var text = buffer.empty(h, 65536);

        // Read every file named, or standard input when none are.
        var named = 0;
        borrow args as &g in {
            named = arg_count(g) - 1;
            borrow fs as &f in {
                var n = 1;
                while n < arg_count(g) {
                    let (grown, got) = read_file(h, f, arg(g, n), text);
                    text = grown;
                    if got == 0 - 2 {
                        // Larger than `read_file` can grow to hold. GNU
                        // has no such case -- it spills to disk -- so
                        // there is no status of its to match, and this
                        // takes one of its own rather than hiding
                        // inside the one for a file that is not there.
                        status = 3;
                        borrow mut io as &!i in {
                            io.error_all(i, "sort: file too large: ");
                            io.error_all(i, arg(g, n));
                            io.error_all(i, "\n");
                        }
                    }
                    if got == 0 - 1 {
                        // GNU writes the failing path to standard error
                        // and exits 2, and now so does this
                        // (`docs/standard-error.md` §7). The *reason* is
                        // still missing -- GNU names the errno string and
                        // `fs_read` answers `-1` with nothing attached --
                        // which is §6's open row and
                        // `file-handles.md` §3's to close.
                        status = 2;
                        borrow mut io as &!i in {
                            io.error_all(i, "sort: cannot read: ");
                            io.error_all(i, arg(g, n));
                            io.error_all(i, "\n");
                        }
                    }
                    n = n + 1;
                }
            }
        }
        if named == 0 {
            borrow mut io as &!i in {
                text = read_stdin(h, i, text);
            }
        }

        if status == 0 {
            var starts = vec.empty(h, 1024, 0);
            var lengths = vec.empty(h, 1024, 0);
            var count = 0;
            borrow text as &t in {
                let (s, l) = find_lines(h, buffer.bytes(t), starts, lengths);
                starts = s;
                lengths = l;
            }
            borrow starts as &s in {
                count = vec.size(s);
            }

            // The permutation and its scratch, both filled to `count` so
            // every index a merge writes already exists.
            var order = vec.empty(h, count + 1, 0);
            var scratch = vec.empty(h, count + 1, 0);
            var n = 0;
            while n < count {
                order = vec.push(h, order, n);
                scratch = vec.push(h, scratch, 0);
                n = n + 1;
            }

            borrow text as &t in {
                borrow starts as &p in {
                    borrow lengths as &q in {
                        borrow mut order as &!o in {
                            borrow mut scratch as &!c in {
                                msort(buffer.bytes(t), p, q, o, c, 0, count);
                            }
                        }
                        borrow order as &o in {
                            borrow mut io as &!i in {
                                write_lines(i, buffer.bytes(t), p, q, o, count);
                            }
                        }
                    }
                }
            }

            vec.drop(h, scratch);
            vec.drop(h, order);
            vec.drop(h, lengths);
            vec.drop(h, starts);
        }

        buffer.drop(h, text);
    }

    release(args);
    release(fs);
    release(io);
    release(heap);
    return status;
}
