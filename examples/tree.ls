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
// error*, not a leak a profiler finds next month.
//
// **And the tree can be read without being spent.** When this example was
// written it could not: `match` required ownership, so the only walk
// available was the one that freed. `docs/reading-references.md` closed
// that — matching a *reference* binds each payload as a reference into the
// tree, so `contains`, `deepest` and `tally` below take `&t Tree`, touch no
// capability at all, and leave the tree exactly as owned as they found it.
// Their rows are `[]`, which is the strongest available statement that they
// free nothing.
//~ STDOUT 1 3 4 5 7 8 9
//~ STDOUT sum 37 count 7 depth 3
//~ STDOUT has 4: 1  has 6: 0  deepest 9
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

fn yes_no(b: bool) -> [] int {
    if b {
        return 1;
    }
    return 0;
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
// Reading, without spending
// ---------------------------------------------------------------------

// Every function in this section takes `&t Tree` and returns an ordinary
// value. None of them can free anything: their rows are `[]`, so they hold
// no capability, and their bindings are references into a tree somebody
// else owns.
//
// `left` and `right` come back as `&t Box[Tree]`. `contents` follows each
// box to `&t Tree`, for exactly as long as `t` lasts, and the recursion
// borrows no longer than this frame does.
fn contains[&t](tree: &t Tree, wanted: int) -> [] bool {
    match tree {
        Tree::Leaf => { return false; }
        Tree::Node(left, value, right) => {
            // `value` is `&t int`, so `*value` reads it.
            if *value == wanted {
                return true;
            }
            if wanted < *value {
                return contains(contents(left), wanted);
            }
            return contains(contents(right), wanted);
        }
    }
}

// The right spine, which in a search tree is the largest value.
fn deepest[&t](tree: &t Tree, fallback: int) -> [] int {
    match tree {
        Tree::Leaf => { return fallback; }
        Tree::Node(_, value, right) => { return deepest(contents(right), *value); }
    }
}

fn tally[&t](tree: &t Tree) -> [] Walk {
    match tree {
        Tree::Leaf => { return Walk { sum: 0, count: 0, depth: 0 }; }
        Tree::Node(left, value, right) => {
            let low = tally(contents(left));
            let high = tally(contents(right));
            return Walk {
                sum: low.sum + *value + high.sum,
                count: low.count + high.count + 1,
                depth: larger(low.depth, high.depth) + 1,
            };
        }
    }
}

// ---------------------------------------------------------------------
// Walking one last time, which is what frees
// ---------------------------------------------------------------------

// In-order traversal, and the one that ends the tree. It still prints and
// still counts, because it is a convenient place to, but it no longer has
// to: the section above can answer any of those questions without spending
// anything.
//
// `left` and `right` are the boxes the match produced — the *boxes*, not
// references to them, because this match takes the tree by value. `unbox`
// frees each node and yields what it held, and the recursion does the same
// to that. Exactly one `free` per node, and the checker is what guarantees
// it.
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

    // Read it, repeatedly, without spending it. Each of these borrows the
    // tree and hands it back.
    var has_four = false;
    var has_six = false;
    var biggest = 0;
    var counted = 0;
    borrow tree as &t in {
        has_four = contains(t, 4);
        has_six = contains(t, 6);
        biggest = deepest(t, 0);
        counted = tally(t).count;
    }

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

    // The read-only pass agreed with the consuming one about the count,
    // which is the point: both walked the same tree, and only one of them
    // was allowed to end it.
    write_all(io, "has 4: ");
    print_nat(io, yes_no(has_four));
    write_all(io, "  has 6: ");
    print_nat(io, yes_no(has_six));
    write_all(io, "  deepest ");
    print_nat(io, biggest);
    putchar(io, 10);
    return found.sum + counted - found.count;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
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
