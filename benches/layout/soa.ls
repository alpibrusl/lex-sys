// The same computation with the same data, transposed by hand: three
// arrays instead of one array of triples. Reads 8 bytes per element.
// This is the transform a compiler could do and C cannot.
import std.io;

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(fs); release(ffi);
    let n = 4000000;
    var total = 0;
    borrow mut heap as &!h in {
        let xs = box_slice(h, n, 1);
        let ys = box_slice(h, n, 2);
        let zs = box_slice(h, n, 3);
        borrow xs as &p in {
            let view = contents(p);
            var round = 0;
            while round < 8 {
                var i = 0;
                while i < len(view) {
                    total = total + view[i];
                    i = i + 1;
                }
                round = round + 1;
            }
        }
        unbox_slice(h, zs);
        unbox_slice(h, ys);
        unbox_slice(h, xs);
    }
    release(heap);
    borrow mut io as &!i in { io.print_int(i, total); io.newline(i); }
    release(io);
    return 0;
}
