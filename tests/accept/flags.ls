// `docs/flags.md` §2 — the nine shapes an argument comes in, printed.
//
// Two jobs in one file. Run with no arguments it prints nothing and
// exits 0, which is what the accept harness checks; run by
// `every_argument_shape` with each spelling in turn, its output *is*
// the table in §2. The alternative was a fixture that proves the
// module compiles and a second program that proves it works.
//
// It asks for a value after `-d` and `--delimiter` and nowhere else,
// which is the whole of §3's protocol: the parser reports a shape and
// the program decides whether that shape has a value behind it.
//~ EXIT 0

import std.flags;
import std.io;

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(ffi);
    release(fs);
    release(heap);

    borrow mut io as &!i in {
        var c = flags.start();
        borrow args as &g in {
            var going = true;
            while going {
                let (next, step) = flags.step(g, c);
                c = next;
                match step {
                    flags.Arg::Short(letter) => {
                        if letter == 'd' {
                            let (after, given) = flags.value(g, c);
                            c = after;
                            io.write_all(i, "short d=");
                            io.write_all(i, given);
                        } else {
                            io.write_all(i, "short ");
                            putchar(i, letter);
                        }
                        io.newline(i);
                    }
                    flags.Arg::Long(name) => {
                        if flags.named(name, "delimiter") {
                            let (after, given) = flags.value(g, c);
                            c = after;
                            io.write_all(i, "long delimiter=");
                            io.write_all(i, given);
                        } else {
                            io.write_all(i, "long ");
                            io.write_all(i, name);
                        }
                        io.newline(i);
                    }
                    flags.Arg::Operand(text) => {
                        io.write_all(i, "operand ");
                        io.write_all(i, text);
                        io.newline(i);
                    }
                    flags.Arg::Done => {
                        going = false;
                    }
                }
            }
        }
    }

    release(args);
    release(io);
    return 0;
}
