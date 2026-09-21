// pipeline.ls — M2 as *one* system, in a program that computes something.
//
// `tour.ls` shows each feature on its own. This shows them working together,
// which is the actual claim: linear ownership, lexical regions, effect rows
// and capabilities are not four systems that coexist, they are one.
//
// A run of jobs is admitted against a budget. Each job is a linear resource
// that must be finished exactly once — completed or cancelled, never both,
// never neither. Deciding which needs the job's cost, and reading a field of
// something you own without spending it is a borrow. The running tally lives
// in an arena. The overrun is computed by libc, through a capability that
// names libc and nothing else. And printing needs the console capability
// `main` was handed, threaded down every frame that prints.
//
// Delete any one of those and the program stops compiling. That is the point.
//~ STDOUT jobs: 4 done, 2 cancelled
//~ STDOUT spent: 49 of 50
//~ STDOUT headroom: 1
//~ EXIT 0

// ---------------------------------------------------------------- libc ----
// §8.4: the declaration is the only place this signature is written, the
// capability is the only way to reach it, and the row names the library.
extern fn labs[&f](ffi: &f Ffi("libc"), n: int) -> [ffi("libc")] int;

// ------------------------------------------------------------- the job ----
// §3: `res`, so it is linear. Exactly one use, no implicit copy, and no
// implicit discard — a job that fell off the end of a scope unfinished is a
// compile error rather than a silently dropped unit of work.
res struct Job {
    id: int,
    cost: int,
}

fn submit(id: int, cost: int) -> [] Job {
    return Job { id: id, cost: cost };
}

// §5: reading a field of an owned `res` value is refused, because a
// non-owning read is a borrow. So the cost is read through a reference, and
// the job is still whole afterwards.
fn cost_of[&r](j: &r Job) -> [] int {
    return j.cost;
}

// The two terminal consumers. §4.1: a resource is destroyed by naming the
// function that knows how, and that function ends in taking it apart. There
// is no destructor, because a destructor is code at a point nobody wrote —
// which would make the effect row on the enclosing function a lie.
fn complete(j: Job) -> [] int {
    let Job { id, cost } = j;
    return cost;
}

fn cancel(j: Job) -> [] int {
    let Job { id, cost } = j;
    return 0;
}

// ------------------------------------------------------------- output -----
// Every one of these declares `[io_write]` and takes an `&!i Io` it did not create.
// Reading the row and reading the parameter list are the same act.

fn space[&i](io: &!i Io) -> [io_write] int {
    return putchar(io, 32);
}

fn newline[&i](io: &!i Io) -> [io_write] int {
    return putchar(io, 10);
}

fn print_nat[&i](io: &!i Io, n: int) -> [io_write] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, 48 + n % 10);
}

// ------------------------------------------------------------ the tally ---
// Scratch state for one run. It lives in an arena: bump-allocated, released
// wholesale when the block closes, and `val` — §6.1 keeps anything with an
// obligation out of a chunk that is reclaimed rather than consumed.
struct Tally {
    spent: int,
    done: int,
    dropped: int,
}

// Written against a *shared* reference and called below on what the arena
// handed back as unique: `&!r T` is `&r T` plus permission to write.
fn spent_of[&r](t: &r Tally) -> [] int {
    return t.spent;
}

// One admission decision. The job arrives owned, and leaves consumed on both
// paths — §4.2, the branches agree about what is live at the merge point.
fn admit[&t](job: Job, tally: &!t Tally, budget: int) -> [] int {
    var cost = 0;
    borrow job as &j in {
        cost = cost_of(j);
    }
    if tally.spent + cost <= budget {
        tally.spent = tally.spent + complete(job);
        tally.done = tally.done + 1;
        return 1;
    }
    cancel(job);
    tally.dropped = tally.dropped + 1;
    return 0;
}

// ---------------------------------------------------------------- run -----
// The row says everything this does: it reaches libc, and it prints. It can
// do nothing else, because it was handed nothing else.
fn run[&f, &i](libc: &f Ffi("libc"), io: &!i Io, budget: int) -> [io_write, ffi("libc")] int {
    var headroom = 0;
    var done = 0;
    var dropped = 0;
    var spent = 0;

    region a {
        let tally = alloc[a](Tally { spent: 0, done: 0, dropped: 0 });

        var id = 1;
        while id <= 6 {
            // A fresh linear job per iteration, finished inside the same
            // iteration. §4.3: a loop body may not consume an *outer*
            // binding, and this consumes only what it made.
            admit(submit(id, id * 17 % 23), tally, budget);
            id = id + 1;
        }

        done = tally.done;
        dropped = tally.dropped;
        spent = spent_of(tally);
        // libc computes the magnitude. The capability is checked and then
        // erased: what `labs` receives is the integer and nothing else.
        headroom = labs(libc, budget - spent);
    }
    // The arena is gone by here, in one call. Everything above survives
    // because it is `int` — nothing whose type mentions `a` may leave.

    putchar(io, 106); putchar(io, 111); putchar(io, 98); putchar(io, 115);
    putchar(io, 58); space(io);                                    // "jobs: "
    print_nat(io, done);
    space(io); putchar(io, 100); putchar(io, 111); putchar(io, 110); putchar(io, 101);
    putchar(io, 44); space(io);                                    // " done, "
    print_nat(io, dropped);
    space(io); putchar(io, 99); putchar(io, 97); putchar(io, 110); putchar(io, 99);
    putchar(io, 101); putchar(io, 108); putchar(io, 108); putchar(io, 101);
    putchar(io, 100);                                              // " cancelled"
    newline(io);

    putchar(io, 115); putchar(io, 112); putchar(io, 101); putchar(io, 110);
    putchar(io, 116); putchar(io, 58); space(io);                  // "spent: "
    print_nat(io, spent);
    space(io); putchar(io, 111); putchar(io, 102); space(io);      // " of "
    print_nat(io, budget);
    newline(io);

    putchar(io, 104); putchar(io, 101); putchar(io, 97); putchar(io, 100);
    putchar(io, 114); putchar(io, 111); putchar(io, 111); putchar(io, 109);
    putchar(io, 58); space(io);                                    // "headroom: "
    print_nat(io, headroom);
    newline(io);

    return done - 4 + dropped - 2 + headroom - 1;
}

fn main(world: World) -> [] int {
    // §8.2: the one place authority enters a program. `split` consumes the
    // `World`, and there is no other way to obtain a capability.
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // This program touches no files, so that authority ends here.
    release(heap);
    release(fs);
    // §7.4: attenuation, one way only. From here this program reaches libc
    // and no other library, whatever the rest of it does.
    let libc = narrow(ffi, "libc");

    var status = 0;
    borrow libc as &f in {
        borrow mut io as &!i in {
            status = run(f, i, 50);
        }
    }

    // Both are resources and both are destroyed exactly once. `main`'s own
    // row is `[]` — not because it does nothing, but because it *owns* the
    // authority rather than borrowing it, and that is already visible in the
    // parameter list.
    release(libc);
    release(io);
    return status;
}
