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

fn digit[&i](io: &!i Io, n: int) -> [io] int {
    return putchar(io, 48 + n);
}

fn run[&i](io: &!i Io) -> [io] int {
    digit(io, identity(7));                                  // 7
    let p = Pair { first: 1, second: true };
    let q = swap(p);                                     // Pair[bool, int]
    digit(io, q.second);                                     // 1
    if q.first { digit(io, 9); } else { digit(io, 0); }          // 9

    let some: Option[int] = Option::Some(5);
    let none: Option[int] = Option::None;
    digit(io, unwrap_or(some, 0));                           // 5
    digit(io, unwrap_or(none, 3));                           // 3

    // The same generic function at a second type.
    let flag: Option[bool] = Option::Some(false);
    if unwrap_or(flag, true) { digit(io, 1); } else { digit(io, 2); }   // 2
    putchar(io, 10);
    return 0;
}

fn main(world: World) -> [] int {
    // §8.2: the runtime hands over exactly one `World`, and `split` consumes
    // it. There is no other way to obtain a capability.
    let Split { io } = split(world);
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
