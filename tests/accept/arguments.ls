// `docs/arguments.md` §3: `argc`, the program name, and a loop over the
// rest.
//
// A test harness runs this with no arguments, so what it prints is the
// count and nothing else -- which is itself the thing worth checking: a
// program started with no arguments still has one, its own name, exactly
// as C hands it over. There is no translation here and `arg(a, 0)` is not
// hidden (§3), because hiding it would be a convenience the program
// cannot see through.
//
// The conformance suite is where real arguments are passed; a fixture
// cannot be given any.
//~ STDOUT 1
//~ STDOUT named: 1
//~ EXIT 0

fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io_write] int {
    var n = 0;
    while n < len(s) {
        putchar(io, int_of(s[n]));
        n = n + 1;
    }
    return len(s);
}

fn print_nat[&i](io: &!i Io, n: int) -> [io_write] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, 48 + n % 10);
}

// The row is the documentation: `[args, io_write]` says this function reads the
// command line *and* writes to the console, and a caller holding neither
// capability cannot reach it. That visibility is the whole reason reading
// argv is an effect (§2) -- not that it is dangerous, but that a function
// whose behaviour depends on the command line should say so.
fn report[&a, &i](args: &a Args, io: &!i Io) -> [args, io_write] int {
    let count = arg_count(args);
    print_nat(io, count);
    putchar(io, 10);

    // `arg(a, 0)` is the program name. Its length is a real number of
    // bytes -- C's NUL is an artifact of the interface, not part of the
    // value (§3.2) -- so a program name is never empty.
    write_all(io, "named: ");
    let name = arg(args, 0);
    if len(name) > 0 {
        print_nat(io, 1);
    } else {
        print_nat(io, 0);
    }
    putchar(io, 10);

    // Everything after the program name, if there is any. With no
    // arguments this loop does not run.
    var n = 1;
    while n < count {
        write_all(io, arg(args, n));
        putchar(io, 10);
        n = n + 1;
    }
    return count;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(ffi);
    release(fs);
    release(heap);

    var status = 0;
    // Shared, like `Ffi` and `Fs` and unlike `Io` and `Heap`: reading argv
    // changes nothing, and two readers at once are the same as one.
    borrow args as &a in {
        borrow mut io as &!i in {
            status = report(a, i);
        }
    }
    release(args);
    release(io);
    return status - 1;
}
