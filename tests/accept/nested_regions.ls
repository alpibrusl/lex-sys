// An inner region reading an outer one, and a `where` clause that holds.
//
// §5.2's relation is a stack: the outer block lexically encloses the inner
// one, so `outer` outlives `inner` and a `&outer` reference may be used where
// a `&inner` one is expected. Nothing else coerces, and the referent never
// changes.
//~ STDOUT 126
//~ EXIT 0

struct Bytes {
    len: int,
}

fn len_of[&r](b: &r Bytes) -> [] int {
    return b.len;
}

// `src <= dst`: whatever `src` is, `dst` outlives it. The call site
// discharges that with the same lookup the body would use.
fn merged[&dst, &src where src <= dst](d: &dst Bytes, s: &src Bytes) -> [] int {
    return len_of(d) * 10 + len_of(s);
}

fn run[&i](io: &!i Io) -> [io] int {
    let big = Bytes { len: 1 };
    let small = Bytes { len: 2 };

    borrow big as &outer in {
        borrow small as &inner in {
            // A reference from the enclosing block, used inside this one.
            putchar(io, 48 + len_of(outer));
            putchar(io, 48 + len_of(inner));
            // And the declared relation, satisfied: `outer` outlives `inner`.
            putchar(io, 48 + merged(outer, inner) / 2);
        }
    }

    putchar(io, 10);
    return 0;
}

fn main(world: World) -> [] int {
    // §8.2: the runtime hands over exactly one `World`, and `split` consumes
    // it. There is no other way to obtain a capability.
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // This program touches no files, so that authority ends here.
    release(heap);
    release(fs);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    var status = 0;
    // Threaded by borrow, not by move: a callee should not consume its
    // caller's authority.
    borrow mut io as &!i in {
        status = run(i);
    }
    // Authority is a resource, so it is destroyed exactly once. A program
    // that forgets this does not compile.
    release(io);
    return status;
}
