//~ STDOUT boxed 41
//~ STDOUT plain 7
//~ STDOUT fallback 3
//~ EXIT 0

// `docs/mode-polymorphism.md` §1 and §3.1 — what the language can
// actually do, which turned out to be more than §12 of
// `linearity-and-effects.md` claimed.
//
// `Holder[T]` is used at a **resource** type and at a copyable one in
// the same program, and `wrap`/`unwrap` serve both. Monomorphisation
// checks each copy at the type it was instantiated at, so `Option[T]`,
// `Result[T]` and `Vec[T]` over a resource type have been expressible
// since M2 -- `docs/standard-library.md` said otherwise and was wrong.
//
// What is new is that the signatures now *say* which modes they work
// at, and are checked once:
//
//   * `wrap` and `unwrap` are **unbounded**, so they are checked as
//     though `T` were `res`. They pass, so they are safe at every
//     instantiation -- and this file uses both.
//
//   * `or_else` is `[T: val]`, because it drops one of two values on
//     each path. For a linear `T` that is a leak, and the bound is how
//     the signature says so instead of leaving a caller to find out.

import std.io;

res struct Ticket {
    serial: int,
}

res struct Holder[T] {
    held: T,
    tag: int,
}

enum Option[T] {
    None,
    Some(T),
}

fn wrap[T](value: T, tag: int) -> [] Holder[T] {
    return Holder { held: value, tag: tag };
}

fn unwrap[T](h: Holder[T]) -> [] T {
    let Holder { held, tag } = h;
    return held;
}

fn or_else[T: val](o: Option[T], fallback: T) -> [] T {
    match o {
        Option::None => {
            return fallback;
        }
        Option::Some(v) => {
            return v;
        }
    }
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);

    // The same generic container at a `res` type...
    let held = wrap(Ticket { serial: 41 }, 1);
    let ticket = unwrap(held);
    let Ticket { serial } = ticket;

    // ...and at a `val` one, in the same program.
    let number = unwrap(wrap(7, 0));

    // And a `val`-bounded function at a copyable type, which is what it
    // is for.
    let chosen = or_else(Option::None, 3);

    borrow mut io as &!i in {
        io.write_all(i, "boxed ");
        io.print_nat(i, serial);
        io.newline(i);
        io.write_all(i, "plain ");
        io.print_nat(i, number);
        io.newline(i);
        io.write_all(i, "fallback ");
        io.print_nat(i, chosen);
        io.newline(i);
    }
    release(io);
    return serial + number + chosen - 51;
}
