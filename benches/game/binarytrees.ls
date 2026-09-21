// binary-trees, from the Computer Language Benchmarks Game.
//
// Allocation and nothing else: build a great many short-lived trees,
// walk each one to a checksum, free it. The benchmark exists to stress
// an allocator, which here is `box` and `unbox` over a `Heap`.
//
// The shape is `std.list`'s: a recursive enum whose recursive position
// is a `Box`, which `heap.md` §4 is what allows a type to contain
// itself. Freeing is explicit and exact — `unbox` on every node, on
// every path — because there is no collector to do it later and
// linearity will not let the program forget.
//
//~ STDOUT stretch tree of depth 11	 check: 4095
//~ STDOUT 1024	 trees of depth 4	 check: 31744
//~ STDOUT 256	 trees of depth 6	 check: 32512
//~ STDOUT 64	 trees of depth 8	 check: 32704
//~ STDOUT 16	 trees of depth 10	 check: 32752
//~ STDOUT long lived tree of depth 10	 check: 2047
//~ EXIT 0

import std.bytes;
import std.io;

// A leaf is `Empty`; every other node has two children. The benchmark
// builds complete trees, so a node either has both or neither.
enum Tree {
    Leaf,
    Node(Box[Tree], Box[Tree]),
}

fn build[&h](heap: &!h Heap, depth: int) -> [heap] Tree {
    if depth <= 0 {
        return Tree::Leaf;
    }
    let left = build(heap, depth - 1);
    let right = build(heap, depth - 1);
    return Tree::Node(box(heap, left), box(heap, right));
}

// Count the nodes and free them on the way back up: one walk, not two,
// because a second walk would need the tree to still exist.
fn check_and_free[&h](heap: &!h Heap, tree: Tree) -> [heap] int {
    match tree {
        Tree::Leaf => {
            return 1;
        }
        Tree::Node(left, right) => {
            let l = unbox(heap, left);
            let r = unbox(heap, right);
            return 1 + check_and_free(heap, l) + check_and_free(heap, r);
        }
    }
}

fn row[&i](out: &!i Io, count: int, depth: int, total: int) -> [io_write] int {
    io.print_int(out, count);
    io.write_all(out, "\t trees of depth ");
    io.print_int(out, depth);
    io.write_all(out, "\t check: ");
    io.print_int(out, total);
    io.newline(out);
    return 0;
}

// The benchmark's `N`, from the command line, with the verified default
// this file's header states. `std.bytes` has `digit_of` and no whole
// number parser (`standard-library.md` §3.1), so it is four lines here
// rather than a library addition nothing else has asked for.
fn size_from[&g](args: &g Args, fallback: int) -> [args] int {
    if arg_count(args) < 2 {
        return fallback;
    }
    let text = arg(args, 1);
    var value = 0;
    var i = 0;
    while i < len(text) {
        let digit = bytes.digit_of(int_of(text[i]));
        if digit < 0 {
            return fallback;
        }
        value = value * 10 + digit;
        i = i + 1;
    }
    if value <= 0 {
        return fallback;
    }
    return value;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(fs);
    release(ffi);

    let mindepth = 4;
    var n = 10;
    borrow args as &g in {
        n = size_from(g, 10);
    }
    release(args);
    var maxdepth = n;
    if mindepth + 2 > maxdepth {
        maxdepth = mindepth + 2;
    }
    let stretch = maxdepth + 1;

    borrow mut heap as &!h in {
        borrow mut io as &!i in {
            // One tree deeper than any other, built and freed first, so
            // the allocator has seen its high-water mark before the
            // timed part starts.
            let tree = build(h, stretch);
            io.write_all(i, "stretch tree of depth ");
            io.print_int(i, stretch);
            io.write_all(i, "\t check: ");
            io.print_int(i, check_and_free(h, tree));
            io.newline(i);

            // The long-lived tree is built now and freed last, so every
            // short-lived tree below is allocated while it is still
            // held.
            let longlived = build(h, maxdepth);

            var depth = mindepth;
            while depth <= maxdepth {
                var iterations = 1;
                var shift = maxdepth - depth + mindepth;
                while shift > 0 {
                    iterations = iterations * 2;
                    shift = shift - 1;
                }
                var total = 0;
                var round = 0;
                while round < iterations {
                    let one = build(h, depth);
                    total = total + check_and_free(h, one);
                    round = round + 1;
                }
                row(i, iterations, depth, total);
                depth = depth + 2;
            }

            io.write_all(i, "long lived tree of depth ");
            io.print_int(i, maxdepth);
            io.write_all(i, "\t check: ");
            io.print_int(i, check_and_free(h, longlived));
            io.newline(i);
        }
    }

    release(heap);
    release(io);
    return 0;
}
