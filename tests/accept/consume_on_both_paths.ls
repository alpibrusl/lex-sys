// Branch agreement (§4.2). Every arm that reaches the merge point must agree
// about what is live, so the consumption is written out on each one. That is
// verbose, and deliberately so: `defer` is the obvious sugar and it is left
// out of M2 so the expansion and the checker are not debugged at once.
//
// An arm that returns is not at the merge point, so it does not have to agree
// with anything -- `take` below consumes on one path and returns on the other.
//~ STDOUT 5379
//~ EXIT 0

res struct File {
    fd: int,
}

fn open(fd: int) -> [] File {
    return File { fd: fd };
}

fn close(f: File) -> [] int {
    let File { fd } = f;
    return fd;
}

// Both arms consume: the join agrees that `f` is spent.
fn either(f: File, flag: bool) -> [] int {
    if flag {
        return close(f);
    } else {
        let fd = close(f);
        return fd + 1;
    }
}

// One arm consumes and returns; the other falls through with `f` still live
// and consumes it after. Divergence is what lets these disagree.
fn take(f: File, flag: bool) -> [] int {
    if flag {
        return close(f) + 2;
    }
    return close(f) + 4;
}

// A `match` is a branch too, and its arms bind the payload they take apart.
enum Slot {
    Empty,
    Full(File),
}

fn drain(s: Slot) -> [] int {
    match s {
        Slot::Empty => {
            return 0;
        }
        Slot::Full(f) => {
            return close(f);
        }
    }
}

fn run[&i](io: &!i Io) -> [io] int {
    putchar(io, 48 + either(open(5), true));
    putchar(io, 48 + either(open(2), false));
    putchar(io, 48 + take(open(5), true));
    putchar(io, 48 + take(open(5), false));
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
