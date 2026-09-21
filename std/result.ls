module std.result;

// `std.result` — an answer, or why there isn't one.
//
// Two type parameters, and the interesting part is that they need not
// be at the same mode. `Result[Ticket, int]` is a resource in the `Ok`
// arm and an ordinary integer in the `Err` arm, and the *type* is a
// resource either way — the mode is a fact about the declaration, not
// about which variant a particular value happens to be in. A program
// holding a `Result[Ticket, int]` that turned out to be `Err` still has
// to consume it, and consuming it is a `match`, which is free.
//
// That is the whole reason this is not `Option` with a payload on the
// empty side: the error carries something, and what it carries has its
// own mode.

pub enum Result[T, E] {
    Ok(T),
    Err(E),
}

// The answer, or the fallback.
//
// `[T: val]` for `unwrap_or`'s reason, and **`[E: val]`** for a second
// one worth separating: the `Ok` arm drops the error. Two parameters,
// two bounds, each earned by a different line of the body.
pub fn unwrap_or[T: val, E: val](r: Result[T, E], fallback: T) -> [] T {
    match r {
        Result::Ok(v) => { return v; }
        Result::Err(_) => { return fallback; }
    }
}

// Did it work?
//
// By reference, so it works over a resource in either position.
pub fn is_ok[T, E, &r](r: &r Result[T, E]) -> [] bool {
    match r {
        Result::Ok(_) => { return true; }
        Result::Err(_) => { return false; }
    }
}

pub fn is_err[T, E, &r](r: &r Result[T, E]) -> [] bool {
    return is_ok(r) == false;
}
