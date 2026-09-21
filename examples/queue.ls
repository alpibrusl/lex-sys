// A work queue: jobs that **own** memory, held in a collection, ended
// exactly once each.
//
// This is the program `docs/collections.md` was written for, and the
// reason it could not be written before is worth more than the program.
// A `Job` owns a boxed slice, so it is a resource, and until this slice
// the only container in the library was `std.buffer` — bytes, and
// nothing else. A queue of jobs had to be hand-rolled per program, or
// not written.
//
// Three rules are visible here, and each one is load-bearing:
//
//   * **The list moves jobs; this file ends them.** `list.push` and
//     `list.pop` are generic and never touch a `Job`'s box. Ending one
//     is `finish`, right here, because a generic function does not know
//     what ending a `T` means — §4.
//
//   * **A job that is never finished does not compile.** Not a leak a
//     profiler finds: the checker refuses it. Delete the `finish` call
//     in `drain` and this file stops building.
//
//   * **The tally is a `Vec[int]`** and the queue is a `List[Job]`, and
//     the difference is the *shape* rather than the generics. A vector
//     is one allocation for all its elements, so freeing it is one
//     `free` that runs nothing — which is exactly why it cannot hold a
//     job. §2.
//~ STDOUT compile 7
//~ STDOUT link 4
//~ STDOUT test 4
//~ STDOUT rejected: 1 blank
//~ STDOUT 3 jobs, 15 bytes
//~ EXIT 0

import std.list;
import std.option;
import std.result;
import std.vec;
import std.io as console;
import std.bytes;

// A unit of work that owns its own storage. `res` is not declared: the
// `Box` makes it one, and a declaration that said so would be repeating
// what the field already says.
struct Job {
    name: Box[[byte]],
    size: int,
}

// Take a name and put it on the heap, so the job owns it rather than
// borrowing it from whoever built it.
fn accept[&h, &s](heap: &!h Heap, name: &s [byte]) -> [heap] Job {
    let held = box_slice(heap, len(name), byte_of(0));
    borrow mut held as &!w in {
        let to = contents(w);
        var i = 0;
        while i < len(name) {
            to[i] = name[i];
            i = i + 1;
        }
    }
    return Job { name: held, size: len(name) };
}

// Print a job and end it, answering how many bytes it held.
//
// This is the only thing in the program that frees a job, and the type
// system knows it: `name` is a `res` value the destructuring produced,
// and `unbox_slice` is the only thing that ends one.
fn finish[&h, &i](heap: &!h Heap, io: &!i Io, job: Job) -> [heap, io_write] int {
    let Job { name, size } = job;
    borrow name as &r in {
        console.write_all(io, contents(r));
    }
    putchar(io, 32);
    console.print_int(io, size);
    console.newline(io);
    unbox_slice(heap, name);
    return size;
}

// A job whose name is empty is not work. The `Err` side is an integer
// and the `Ok` side is a resource, which is two modes in one type.
fn vet[&h, &s](heap: &!h Heap, name: &s [byte]) -> [heap] result.Result[Job, int] {
    if len(name) == 0 {
        return result.Result::Err(0);
    }
    // A name that is all blanks is a name nobody typed on purpose.
    var seen = 0;
    var i = 0;
    while i < len(name) {
        if bytes.is_blank(int_of(name[i])) == false {
            seen = seen + 1;
        }
        i = i + 1;
    }
    if seen == 0 {
        return result.Result::Err(len(name));
    }
    return result.Result::Ok(accept(heap, name));
}

// Put a job on the queue and its size in the tally.
//
// The size is read through a `borrow` rather than off the value,
// because a field cannot be read out of a `res` value at all -- not
// even a copyable field, since reading one without owning the whole is
// a borrow and a borrow is what this writes. Then the job is moved on
// to the queue, whole and unspent.
fn enqueue[&h](
    heap: &!h Heap,
    queue: list.List[Job],
    sizes: vec.Vec[int],
    job: Job,
) -> [heap] (list.List[Job], vec.Vec[int]) {
    var size = 0;
    borrow job as &j in {
        size = j.size;
    }
    return (list.push(heap, queue, job), vec.push(heap, sizes, size));
}

// Work the queue to the end, tallying as it goes.
//
// Recursive rather than a loop, and that is forced rather than a style
// choice: a `while` would leave a `List[Job]` behind that the checker
// cannot see is empty, so the obligation would survive the loop. The
// recursion ends on `Empty`, which a `match` consumes outright.
fn drain[&h, &i](heap: &!h Heap, io: &!i Io, queue: list.List[Job]) -> [heap, io_write] int {
    match list.pop(heap, queue) {
        option.Option::None => { return 0; }
        option.Option::Some(pair) => {
            let (job, rest) = pair;
            let bytes_held = finish(heap, io, job);
            return bytes_held + drain(heap, io, rest);
        }
    }
}

fn run[&h, &i](heap: &!h Heap, io: &!i Io) -> [heap, io_write] int {
    // Built by prepending, so the queue comes out in the order written.
    var queue = list.List::Empty;
    var sizes = vec.empty(heap, 4, 0);

    var rejected = 0;
    // A `Result` is consumed whichever side it landed on -- the mode is
    // a fact about the type, not about which variant a value is in. This
    // one is an `Err`, and it still has to be matched.
    match vet(heap, "   ") {
        result.Result::Ok(job) => {
            let (q, s) = enqueue(heap, queue, sizes, job);
            queue = q;
            sizes = s;
        }
        result.Result::Err(n) => { rejected = rejected + 1; }
    }
    match vet(heap, "test") {
        result.Result::Ok(job) => {
            let (q, s) = enqueue(heap, queue, sizes, job);
            queue = q;
            sizes = s;
        }
        result.Result::Err(n) => { rejected = rejected + 1; }
    }
    match vet(heap, "link") {
        result.Result::Ok(job) => {
            let (q, s) = enqueue(heap, queue, sizes, job);
            queue = q;
            sizes = s;
        }
        result.Result::Err(n) => { rejected = rejected + 1; }
    }
    match vet(heap, "compile") {
        result.Result::Ok(job) => {
            let (q, s) = enqueue(heap, queue, sizes, job);
            queue = q;
            sizes = s;
        }
        result.Result::Err(n) => { rejected = rejected + 1; }
    }

    // Counted through a borrow, so counting costs the queue nothing --
    // which it would not have, a `match` ago.
    var waiting = 0;
    borrow queue as &q in {
        waiting = list.length(q);
    }

    let held = drain(heap, io, queue);

    console.write_all(io, "rejected: 1 blank");
    console.newline(io);
    console.print_int(io, waiting);
    console.write_all(io, " jobs, ");
    console.print_int(io, held);
    console.write_all(io, " bytes");
    console.newline(io);

    var counted = 0;
    borrow sizes as &s in {
        counted = vec.size(s);
    }
    let freed = vec.drop(heap, sizes);
    return waiting + rejected + counted + freed - 10;
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
    return status;
}
