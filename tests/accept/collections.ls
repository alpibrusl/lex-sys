// `docs/collections.md`: which collections hold a resource, and which
// one does not.
//
// The whole point of this file is that **nothing here is a new language
// feature**. `Option`, `Result` and `List` over a resource type work
// because monomorphisation checks each instantiation at the type it was
// instantiated at, which has been true since M2 — what was missing was
// a library that said so and a document that had checked.
//
// What is new is the bound on a type *declaration*, and `Vec` is why:
// it owns an allocation, so it is `res`, while its elements have to be
// copyable, because a boxed slice holds `val` data only. `res` and
// `val` in one declaration, about two different things.
//~ STDOUT 14 3 . 2 . 709
//~ STDOUT ok 2
//~ EXIT 0

import std.list;
import std.option;
import std.result;
import std.vec;
import std.io as console;

res struct Ticket {
    serial: int,
}

// A `[T: val]` function naming a `val` aggregate at `T`.
//
// This did not compile before `docs/collections.md` §4: the bound was
// written, and the check that keeps `Wrap`'s own bound read the
// argument against *nothing*, so the rigid `T` came out `res` however
// it was bounded. The bound was unusable for exactly the case it exists
// for.
val struct Wrap[T] {
    held: T,
}

fn rewrap[T: val](w: Wrap[T]) -> [] T {
    let Wrap { held } = w;
    return held;
}

// A list of resources. The library moves the tickets; this function is
// the only thing that ends one, which is `collections.md` §4's rule --
// a generic function can move a `T` and can never end one.
fn spend[&h, &i](heap: &!h Heap, io: &!i Io, held: list.List[Ticket]) -> [heap, io_write] int {
    match list.pop(heap, held) {
        option.Option::None => { return 0; }
        option.Option::Some(pair) => {
            let (head, rest) = pair;
            let Ticket { serial } = head;
            console.print_int(io, serial);
            putchar(io, 32);
            return serial + spend(heap, io, rest);
        }
    }
}

// An `Option` over a resource, and the `match` that consumes it. A
// `None` still has to be consumed -- the mode is a fact about the type,
// not about which variant a value happens to be in.
fn redeem(o: option.Option[Ticket]) -> [] int {
    match o {
        option.Option::None => { return 0; }
        option.Option::Some(t) => {
            let Ticket { serial } = t;
            return serial;
        }
    }
}

// Two parameters at two different modes in one type: the `Ok` is a
// resource, the `Err` is an integer.
fn settle(r: result.Result[Ticket, int]) -> [] int {
    match r {
        result.Result::Ok(t) => {
            let Ticket { serial } = t;
            return serial;
        }
        result.Result::Err(code) => { return 0 - code; }
    }
}

fn run[&h, &i](heap: &!h Heap, io: &!i Io) -> [heap, io_write] int {
    var held = list.List::Empty;
    held = list.push(heap, held, Ticket { serial: 3 });
    held = list.push(heap, held, Ticket { serial: 14 });

    // Counted through a borrow, so counting costs the list nothing.
    var counted = 0;
    borrow held as &l in {
        counted = list.length(l);
    }
    let total = spend(heap, io, held);
    putchar(io, 46);
    putchar(io, 32);
    console.print_int(io, counted);
    putchar(io, 32);
    putchar(io, 46);
    putchar(io, 32);

    // The array-shaped collection, at a copyable element. `get` takes a
    // reference and reads like a getter should -- it used to hand the
    // vector back in a tuple, because a `res` field could not be reached
    // through a reference at all (`docs/reading-references.md` §2.0).
    var v = vec.empty(heap, 2, 0);
    v = vec.push(heap, v, 7);
    v = vec.push(heap, v, 8);
    v = vec.push(heap, v, 9);
    var first = 0;
    var third = 0;
    borrow v as &b in {
        first = vec.get(b, 0);
        third = vec.get(b, 2);
    }
    console.print_int(io, first * 100 + third);
    console.newline(io);
    let freed = vec.drop(heap, v);

    // The resource `Option` and `Result`, and the `val` aggregate at a
    // bounded parameter.
    let kept = redeem(option.Option::Some(Ticket { serial: 1 }));
    let nothing: option.Option[Ticket] = option.Option::None;
    let none = redeem(nothing);
    let good = settle(result.Result::Ok(Ticket { serial: 2 }));
    let bad: result.Result[Ticket, int] = result.Result::Err(5);
    let failed = settle(bad);
    let plain = rewrap(Wrap { held: 4 });

    console.write_all(io, "ok ");
    console.print_int(io, kept + none + good + failed + plain);
    console.newline(io);

    return total + counted + freed;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // Nothing here calls into C, so that authority is dropped at once.
    release(ffi);
    // This program touches no files, so that authority ends here.
    release(fs);

    var status = 0;
    borrow mut heap as &!h in {
        borrow mut io as &!i in {
            status = run(h, i);
        }
    }
    release(heap);
    release(io);
    return status - 22;
}
