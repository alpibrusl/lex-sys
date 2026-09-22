// `docs/file-handles.md`: an open descriptor as a linear value.
//
// The whole milestone in one program. It writes a file with the
// whole-file `fs_write` it already had, then reads it back **through a
// handle** -- which is the shape that could not be written before,
// because `fs_read` wants a buffer sized before the length is known.
//
// Three things this shows that the reject fixtures cannot:
//
//   * a `File` is an ordinary `res` value. `close` consumes it, and
//     forgetting to is `tests/reject/file_handle_unclosed.ls`. The
//     checker needed nothing new, which is §2's claim;
//   * `read` answers a **three-constructor** `Read` rather than a
//     sentinel, so the end of the file and a failure are different arms
//     instead of two meanings of `-1` (§3);
//   * reading past the end answers `End` **again**. §6 settled that
//     against POSIX, and this is why it is a fixture rather than a
//     paragraph.
//~ STDOUT got 12
//~ STDOUT end
//~ STDOUT end again
//~ STDOUT closed
//~ EXIT 0

import std.io;

// The row is `[file_read]` and nothing else: §4.1's label, carrying no
// path, because the path was spent at `open_read` and *that* row names
// the directory. A function handed a handle can read it and cannot say
// where it came from, which is what a descriptor is.
fn step[&h, &b](h: &!h File, into: &!b [byte]) -> [file_read] int {
    match file_read(h, into) {
        Read::Got(n) => {
            return n;
        }
        Read::End => {
            return -1;
        }
        Read::Failed(e) => {
            return 0 - 100 - e;
        }
    }
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    // Everything it allocates is in a region, so the heap ends here too.
    release(heap);

    let path = "/tmp/lex-sys-handle.txt";
    var status = 0;
    region a {
        var buffer = alloc_slice[a](64, byte_of(0));
        borrow fs as &c in {
            fs_write(c, path, "hello handle");
        }

        borrow mut io as &!i in {
            borrow fs as &c in {
                match open_read(c, path) {
                    Opened::Ok(f) => {
                        var file = f;
                        borrow mut file as &!h in {
                            io.write_all(i, "got ");
                            io.print_int(i, step(h, buffer));
                            io.newline(i);
                            io.write_all(i, "end");
                            io.newline(i);
                            // The second one is the fixture's point.
                            if step(h, buffer) == -1 {
                                io.write_all(i, "end again");
                                io.newline(i);
                            }
                        }
                        io.write_all(i, "closed");
                        io.newline(i);
                        status = file_close(file);
                    }
                    Opened::Failed(e) => {
                        status = e;
                    }
                }
            }
        }
    }
    release(io);
    release(fs);
    return status;
}
