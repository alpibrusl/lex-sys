// Generic structs, enums and functions, monomorphised. `unwrap_or` is
// instantiated at both `int` and `bool`, and `Option::None` learns its `T`
// from the annotation on the binding rather than from an argument.
//~ STDOUT 719532
//~ EXIT 0

struct Pair[A, B] { first: A, second: B }

enum Option[T] { None, Some(T) }

fn identity[T](x: T) -> [] T {
    return x;
}

fn swap[A, B](p: Pair[A, B]) -> [] Pair[B, A] {
    return Pair { first: p.second, second: p.first };
}

fn unwrap_or[T](o: Option[T], fallback: T) -> [] T {
    match o {
        Option::None => { return fallback; }
        Option::Some(v) => { return v; }
    }
}

fn digit(n: int) -> [io] int {
    return putchar(48 + n);
}

fn main() -> [io] int {
    digit(identity(7));                                  // 7
    let p = Pair { first: 1, second: true };
    let q = swap(p);                                     // Pair[bool, int]
    digit(q.second);                                     // 1
    if q.first { digit(9); } else { digit(0); }          // 9

    let some: Option[int] = Option::Some(5);
    let none: Option[int] = Option::None;
    digit(unwrap_or(some, 0));                           // 5
    digit(unwrap_or(none, 3));                           // 3

    // The same generic function at a second type.
    let flag: Option[bool] = Option::Some(false);
    if unwrap_or(flag, true) { digit(1); } else { digit(2); }   // 2
    putchar(10);
    return 0;
}
