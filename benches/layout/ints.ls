// A memory-bound traversal of a struct array: the shape where layout is
// the whole cost. Three byte fields, which lex-sys stores as three
// 8-byte leaves and C stores as three bytes.
import std.io;

struct Rgb { r: int, g: int, b: int }

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(fs); release(ffi);

    let n = 4000000;
    var total = 0;
    borrow mut heap as &!h in {
        let pixels = box_slice(h, n, Rgb { r: 1, g: 2, b: 3 });
        borrow pixels as &p in {
            let view = contents(p);
            var round = 0;
            while round < 8 {
                var i = 0;
                while i < len(view) {
                    let px = view[i];
                    total = total + px.r + px.g + px.b;
                    i = i + 1;
                }
                round = round + 1;
            }
        }
        unbox_slice(h, pixels);
    }
    release(heap);

    borrow mut io as &!i in {
        io.print_int(i, total);
        io.newline(i);
    }
    release(io);
    return 0;
}
