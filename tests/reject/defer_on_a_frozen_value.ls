//~ ERROR is frozen by an enclosing `borrow`
//~ RULE borrow-conflict

// `docs/defer.md` §2.1: a `defer` runs at the end of **its own block**,
// which for one written inside a `borrow` is before the borrow ends —
// so the value is still frozen when it runs, and consuming it is
// refused by §5's rule rather than by a rule about `defer`.
//
// This is the case that decided block scope over function scope. Under
// function scope this `defer` would run after the borrow closed, which
// reads as though it ought to work, and then a `defer` written inside a
// `region` could outlive the arena it allocated from.

res struct Ticket {
    fd: int,
}

fn open(n: int) -> [] Ticket {
    return Ticket { fd: n };
}

fn close(f: Ticket) -> [] int {
    let Ticket { fd } = f;
    return fd;
}

fn frozen() -> [] int {
    var f = open(7);
    borrow f as &r in {
        defer close(f);
    }
    return close(f);
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);
    release(io);
    return 0;
}
