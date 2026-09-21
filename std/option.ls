module std.option;

// `std.option` — a value, or nothing.
//
// The first collection here that works at **both modes**, and it needed
// no new feature to do it: `docs/mode-polymorphism.md` established that
// monomorphisation already checks each instantiation at the type it was
// instantiated at, so `Option[int]` is copyable and `Option[Ticket]` is
// a resource the checker will not let a program forget. The mode is
// *computed* from the argument, which is why the declaration writes no
// mode keyword at all.
//
// What the mode decides is not whether the type works but which
// **functions** do. `unwrap_or` has to drop one of two values, so it is
// `[T: val]` and says so; `map_or` would need to call something, and
// there are no closures. What is left over a resource is what moves it
// without ending it, and that is `docs/collections.md` §4's whole rule:
// a generic function can move a `T` and can never end one, because
// ending a `T` is exactly the thing it does not know how to do.

pub enum Option[T] {
    None,
    Some(T),
}

// The value, or the fallback.
//
// `[T: val]` because exactly one of the two arguments comes back and
// the other is dropped, which only a copyable type allows. This is not
// a workaround for a missing feature: it is what the function always
// meant, and the bound moves the refusal to the caller that reached for
// it with a resource rather than into this file.
pub fn unwrap_or[T: val](o: Option[T], fallback: T) -> [] T {
    match o {
        Option::None => { return fallback; }
        Option::Some(v) => { return v; }
    }
}

// Is there a value?
//
// By reference, so it works over a resource: reading `Option[Ticket]`
// this way costs the caller nothing, where taking it by value would
// hand over a ticket the caller still owes. `docs/reading-references.md`
// §3 is what makes the `match` arms bind references rather than move.
pub fn is_some[T, &o](o: &o Option[T]) -> [] bool {
    match o {
        Option::None => { return false; }
        Option::Some(_) => { return true; }
    }
}

pub fn is_none[T, &o](o: &o Option[T]) -> [] bool {
    return is_some(o) == false;
}
