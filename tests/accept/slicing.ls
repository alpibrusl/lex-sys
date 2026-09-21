// `docs/slicing.md`: `s[a..b]`, and the question it turned out to
// answer.
//
// The feature is small — a pointer and a length, which is what a slice
// always was. What is worth reading is `report`, which is §6: there is
// **no `Writer` type** in this language and there should not be one,
// because a function taking one would have to declare the union of what
// every destination could do, on every call. A union row is not an
// exact row, and an exact row is the whole claim.
//
// So the abstraction is the **buffer**. `report` formats and touches
// `[heap]`; printing touches `[io_write]`; writing a file touches
// `[fs_write("/tmp")]`. Three exact rows, one set of bytes, no dispatch
// — and `buffer.bytes` is the operation that could not be written
// before, because a buffer holds more than it uses and nothing could
// say which part was which.
//~ STDOUT hello world wor
//~ STDOUT seen 42 items
//~ EXIT 0

import std.buffer;
import std.bytes;
import std.io as console;

// Format once. The row says this touches the heap and nothing else —
// not the console, not the filesystem, whatever the caller does next.
fn report[&h](heap: &!h Heap, b: buffer.Buffer, n: int) -> [heap] buffer.Buffer {
    var out = buffer.append(heap, b, "seen ");
    out = buffer.push_nat(heap, out, n);
    out = buffer.append(heap, out, " items\n");
    return out;
}

fn run[&h, &i, &f](heap: &!h Heap, io: &!i Io, tmp: &!f Fs("/tmp")) -> [heap, io_write, fs_write("/tmp")] int {
    let text = "hello, world";

    // A literal is a slice, so it ranges like any other.
    console.write_all(io, text[0..5]);
    putchar(io, 32);
    let rest = text[7..12];
    console.write_all(io, rest);
    putchar(io, 32);
    // A slice of a slice: the region rides along, so this is still a
    // reference into the same literal.
    console.write_all(io, rest[0..3]);
    console.newline(io);

    // `starts_with` is `equal` over a subslice now, which is the
    // clearest measure of what this bought: it existed *because* the
    // subslice could not be written.
    var checks = 0;
    if bytes.starts_with(text, "hello") { checks = checks + 1; }
    if bytes.ends_with(text, "world") { checks = checks + 1; }
    if bytes.starts_with(text, "world") == false { checks = checks + 1; }
    // An empty range is legal and empty; a full one is the whole slice.
    if len(text[4..4]) == 0 { checks = checks + 1; }
    if len(text[0..len(text)]) == 12 { checks = checks + 1; }

    var written = 0;
    var b = buffer.empty(heap, 8);
    b = report(heap, b, 42);
    borrow b as &r in {
        // The same bytes, to two destinations, each with its own exact
        // row. Neither `report` nor `bytes` knows either of them exists.
        console.write_all(io, buffer.bytes(r));
        written = fs_write(tmp, "/tmp/lex-sys-slicing.txt", buffer.bytes(r));
    }
    let held = buffer.drop(heap, b);

    return checks + written + held;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);

    let tmp = narrow(fs, "/tmp");
    var status = 0;
    borrow mut heap as &!h in {
        borrow mut io as &!i in {
            borrow mut tmp as &!f in {
                status = run(h, i, f);
            }
        }
    }
    release(tmp);
    release(heap);
    release(io);
    return status - 33;
}
