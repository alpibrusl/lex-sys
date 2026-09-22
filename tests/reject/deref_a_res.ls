//~ ERROR is `res`, so `*` would copy it
//~ RULE linear-value-taken-apart

// `docs/reading-references.md` §3: `*` copies, so what it copies has to be
// `val`.
//
// Copying a `res` out of a reference would leave two values where one
// obligation is owed -- the original, still the owner's, and a duplicate
// nobody is required to consume. That is exactly the hole §4's
// exactly-once rule exists to close, and a new operator does not get an
// exception to it.
//
// A `res` behind a reference is read the way it always was: borrow it
// further, or name a field. Ending one is the owner's business.

res struct Ticket { serial: int }

fn peek[&r](t: &r Ticket) -> [] Ticket {
    return *t;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    return 0;
}
