// §6: allocate, walk, release in O(1).
//
// Three things this shows and the rejects cannot:
//
//   * an arena is a region, and `alloc[a]` hands back `&!a T` -- an ordinary
//     unique reference, so §5's reads and writes work on it unchanged. There
//     is no second notion of "pointer into an arena";
//   * a helper written against `&r` is callable on arena data, because a
//     unique reference is a shared one plus permission to write;
//   * allocation is a bump. The loop allocates eight nodes and the arena's
//     release is still one call, whatever the loop did.
//~ STDOUT 0 1 4 9 16 25 36 49 = 140
//~ STDOUT nested: 7
//~ EXIT 0

struct Node {
    value: int,
}

// Takes a *shared* reference, and is called below on something `alloc`
// handed back as unique. Without that one coercion, nothing written against
// `&r` could ever touch arena data.
fn value_of[&r](n: &r Node) -> [] int {
    return n.value;
}

fn space[&i](io: &!i Io) -> [io] int {
    return putchar(io, 32);
}

fn print_nat[&i](io: &!i Io, n: int) -> [io] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, 48 + n % 10);
}

// Allocate eight squares, reading each one back through a reference as it is
// made. The arena grows by a bump per node and is released once at the end.
fn squares[&i](io: &!i Io) -> [io] int {
    var total = 0;
    region a {
        var i = 0;
        while i < 8 {
            let node = alloc[a](Node { value: i * i });
            // Written through the unique reference the arena handed back...
            node.value = node.value + 0;
            // ...and read through a function that only asked for a shared one.
            if i > 0 {
                space(io);
            }
            print_nat(io, value_of(node));
            total = total + value_of(node);
            i = i + 1;
        }
    }
    putchar(io, 32); putchar(io, 61); putchar(io, 32);   // " = "
    print_nat(io, total);
    putchar(io, 10);
    return total;
}

// Nesting is §5.2's stack again: the inner arena may hold what the outer one
// allocated, because the outer outlives it. The reverse is
// `tests/reject/inner_region_stored_in_outer.ls`.
fn nested[&i](io: &!i Io) -> [io] int {
    var answer = 0;
    region outer {
        let base = alloc[outer](Node { value: 3 });
        region inner {
            let extra = alloc[inner](Node { value: 4 });
            // `base` comes from the enclosing arena, so it is still good here.
            answer = value_of(base) + value_of(extra);
        }
    }
    putchar(io, 110); putchar(io, 101); putchar(io, 115); putchar(io, 116);
    putchar(io, 101); putchar(io, 100); putchar(io, 58); putchar(io, 32);
    print_nat(io, answer);
    putchar(io, 10);
    return answer;
}

fn main(world: World) -> [] int {
    let Split { io, ffi } = split(world);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    var status = 0;
    borrow mut io as &!i in {
        status = squares(i) - 140 + nested(i) - 7;
    }
    release(io);
    return status;
}
