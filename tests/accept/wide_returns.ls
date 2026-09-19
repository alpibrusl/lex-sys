// A value too wide to come back in registers travels through a buffer the
// caller allocates. Two leaves fit on x86-64; anything above that does not,
// and Cranelift refuses outright rather than arranging it for you.
//
// This fixture exists because the limit is otherwise invisible: `Point` (two
// ints) returns fine, and the failure only appears at three.
//~ STDOUT 17321
//~ EXIT 0

struct Three { a: int, b: int, c: int }
struct Nested { left: Three, right: Three, flag: bool }

enum Wide { Small(int), Big(Three) }

fn three(a: int, b: int, c: int) -> Three {
    return Three { a: a, b: b, c: c };
}

fn nest(flag: bool) -> Nested {
    return Nested { left: three(1, 2, 3), right: three(4, 5, 6), flag: flag };
}

fn widen(n: int) -> Wide {
    if n > 5 {
        return Wide::Big(three(n, n, n));
    }
    return Wide::Small(n);
}

fn total(w: Wide) -> int {
    match w {
        Wide::Small(n) => { return n; }
        Wide::Big(t) => { return t.a + t.b + t.c; }
    }
}

fn main() -> int {
    let t = three(1, 2, 3);
    putchar(48 + t.a);                       // 1

    let n = nest(true);
    putchar(48 + n.right.c - 6 + 7);         // 7
    if n.flag {
        putchar(48 + n.left.b + 1);          // 3
    } else {
        putchar(48);
    }

    // The wide path: `Wide::Big` carries a three-field struct, so the enum is
    // four leaves and comes back through the buffer.
    let big = total(widen(7));               // Big(7, 7, 7) -> 21
    putchar(48 + big / 10);                  // 2
    putchar(48 + big % 10);                  // 1
    putchar(10);
    return 0;
}
