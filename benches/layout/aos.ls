// Array of structs, touching ONE field of three. Reads 24 bytes per
// element to use 8 — which is what a struct array costs any language.
import std.io;
struct P { x: int, y: int, z: int }

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(fs); release(ffi);
    let n = 4000000;
    var total = 0;
    borrow mut heap as &!h in {
        let items = box_slice(h, n, P { x: 1, y: 2, z: 3 });
        borrow items as &p in {
            let view = contents(p);
            var round = 0;
            while round < 8 {
                var i = 0;
                while i < len(view) {
                    total = total + view[i].x;
                    i = i + 1;
                }
                round = round + 1;
            }
        }
        unbox_slice(h, items);
    }
    release(heap);
    borrow mut io as &!i in { io.print_int(i, total); io.newline(i); }
    release(io);
    return 0;
}
