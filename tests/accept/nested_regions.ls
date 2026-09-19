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

fn len_of[&r](b: &r Bytes) -> int {
    return b.len;
}

// `src <= dst`: whatever `src` is, `dst` outlives it. The call site
// discharges that with the same lookup the body would use.
fn merged[&dst, &src where src <= dst](d: &dst Bytes, s: &src Bytes) -> int {
    return len_of(d) * 10 + len_of(s);
}

fn main() -> int {
    let big = Bytes { len: 1 };
    let small = Bytes { len: 2 };

    borrow big as &outer in {
        borrow small as &inner in {
            // A reference from the enclosing block, used inside this one.
            putchar(48 + len_of(outer));
            putchar(48 + len_of(inner));
            // And the declared relation, satisfied: `outer` outlives `inner`.
            putchar(48 + merged(outer, inner) / 2);
        }
    }

    putchar(10);
    return 0;
}
