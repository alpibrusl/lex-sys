// `tree.ls` — a binary search tree, which is the shortest honest answer to
// "why does a language need a heap at all".
//
// Everything before M3's heap lived in a frame or an arena, and both are
// *lexical*: a value's lifetime is a block, and §5's escape check exists to
// keep it there. A tree is the standard counterexample. Its shape is
// decided at run time, its nodes outlive the calls that made them, and the
// type is defined in terms of itself — none of which a block can express.
//
// Two things here are worth reading more than the tree itself.
//
// **The type compiles at all.** `enum Tree { Leaf, Node(Tree, int, Tree) }`
// has no finite size and is refused. One `Box` on the path back to itself
// and it has one, because a box is a pointer however large what it points
// at is (`docs/heap.md` §4).
//
// **Nothing here frees anything, and everything is freed.** There is no
// `free` in this file. `unbox` is the only thing that ends a box, a `Box`
// is a `res` value, and §4's rule is that a `res` value is consumed exactly
// once on every path — so a node this program forgot would be a *compile
// error*, not a leak a profiler finds next month. The walk that reads the
// tree and the proof that it released every node are the same code.
//~ STDOUT 1 3 4 5 7 8 9
//~ STDOUT sum 37 count 7 depth 3
//~ EXIT 0

// ---------------------------------------------------------------------
// The type that needed a heap
// ---------------------------------------------------------------------

enum Tree {
    Leaf,
    Node(Box[Tree], int, Box[Tree]),
}

// What one pass over the tree found. Returned by value, so the walk is a
// single traversal rather than three.
struct Walk {
    sum: int,
    count: int,
    depth: int,
}

// ---------------------------------------------------------------------
// Console
// ---------------------------------------------------------------------

fn print_nat[&i](io: &!i Io, n: int) -> [io] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, 48 + n % 10);
}

fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io] int {
    var n = 0;
    while n < len(s) {
        putchar(io, int_of(s[n]));
        n = n + 1;
    }
    return len(s);
}

fn larger(a: int, b: int) -> [] int {
    if a > b {
        return a;
    }
    return b;
}

// ---------------------------------------------------------------------
// Building
// ---------------------------------------------------------------------

// Insert, consuming the tree and handing back the new one.
//
// That signature is not a compromise: `match` takes ownership (§4.1), so a
// tree that is being restructured is a tree being *spent*. Read the row —
// `[heap]` is the whole of what this function can do to the world.
//
// Every path consumes both children exactly once. The `v < value` branch
// unboxes the left child and boxes the result of inserting into it, and
// passes the right one straight through; the other branch does the mirror
// image. Drop either and the program does not compile.
fn insert[&h](heap: &!h Heap, t: Tree, v: int) -> [heap] Tree {
    match t {
        Tree::Leaf => {
            // Two fresh leaves, two allocations, one node.
            return Tree::Node(box(heap, Tree::Leaf), v, box(heap, Tree::Leaf));
        }
        Tree::Node(left, value, right) => {
            if v < value {
                let l = unbox(heap, left);
                return Tree::Node(box(heap, insert(heap, l, v)), value, right);
            }
            let r = unbox(heap, right);
            return Tree::Node(left, value, box(heap, insert(heap, r, v)));
        }
    }
}

// ---------------------------------------------------------------------
// Walking, which is the same thing as freeing
// ---------------------------------------------------------------------

// In-order traversal. It prints the values in sorted order, sums them,
// counts them, measures the depth, and frees every node — in one pass,
// because there is no second pass available: reading a recursive structure
// means consuming it (§4.1).
//
// `left` and `right` are the boxes the match produced. `unbox` frees each
// node and yields what it held, and the recursion does the same to that.
// Exactly one `free` per node, and the checker is what guarantees it.
fn drain[&h, &i](heap: &!h Heap, io: &!i Io, t: Tree, first: bool) -> [heap, io] Walk {
    match t {
        Tree::Leaf => {
            return Walk { sum: 0, count: 0, depth: 0 };
        }
        Tree::Node(left, value, right) => {
            let l = unbox(heap, left);
            let r = unbox(heap, right);

            // Left subtree first, so the values come out in order. Whether
            // *this* node is the first thing printed depends on whether
            // anything was printed to its left.
            let low = drain(heap, io, l, first);
            if first && low.count == 0 {
                print_nat(io, value);
            } else {
                putchar(io, 32);
                print_nat(io, value);
            }
            let high = drain(heap, io, r, false);

            return Walk {
                sum: low.sum + value + high.sum,
                count: low.count + high.count + 1,
                depth: larger(low.depth, high.depth) + 1,
            };
        }
    }
}

fn run[&h, &i](heap: &!h Heap, io: &!i Io) -> [heap, io] int {
    var tree = Tree::Leaf;
    tree = insert(heap, tree, 5);
    tree = insert(heap, tree, 3);
    tree = insert(heap, tree, 8);
    tree = insert(heap, tree, 1);
    tree = insert(heap, tree, 4);
    tree = insert(heap, tree, 7);
    tree = insert(heap, tree, 9);

    // The only path that ends the tree. After this line there is no tree,
    // and there is also nothing left allocated.
    let found = drain(heap, io, tree, true);
    putchar(io, 10);

    write_all(io, "sum ");
    print_nat(io, found.sum);
    write_all(io, " count ");
    print_nat(io, found.count);
    write_all(io, " depth ");
    print_nat(io, found.depth);
    putchar(io, 10);
    return found.sum;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap } = split(world);
    // No foreign calls and no files: `box` and `unbox` reach libc from the
    // backend, the way the arena and the file operations already do, so a
    // program that allocates needs neither of those capabilities.
    release(ffi);
    release(fs);

    var status = 0;
    borrow mut heap as &!h in {
        borrow mut io as &!i in {
            status = run(h, i);
        }
    }

    // Both are resources and both are destroyed exactly once.
    release(heap);
    release(io);
    return status - 37;
}
