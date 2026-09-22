module std.flags;

// `std.flags` — the shape of a command line, and nothing about its meaning.
//
// `docs/flags.md` is the design, and §1 is why this exists: the two
// programs here that parse flags each reimplemented a guess at the
// syntax, between them covered three of its nine shapes, and one of them
// answered `--decode` by encoding and exiting 0. Argument syntax is a
// specification, and a guess at a specification is what this file
// replaces.
//
// §3 is the interface and the reason it is a cursor rather than a table:
// a table has to know which options take values, and to be GNU it then
// has to resolve abbreviations against that table. Here the program says
// when it wants a value, so there is no table to search and no ambiguity
// to resolve.

import std.bytes;

// One argument's shape.
//
// Not a `Result`: nothing here fails. An argument this does not
// recognise is an `Operand`, and whether that is an error is the
// program's question rather than the parser's (§4).
pub enum Arg {
    // A short flag's letter, as a code point: `-d` gives `'d'`.
    Short(int),
    // A long flag's name, without the dashes and without any `=value`.
    Long(&static [byte]),
    // A file, a bare `-`, or anything after `--`.
    Operand(&static [byte]),
    Done,
}

// Where the walk has got to.
//
// Three integers and no reference, so it is `val`: copied at every step
// and reassigned, which is `examples/sort/`'s `(grown, got)` shape and
// needs no borrow through a `match`.
pub struct Cursor {
    // Which argument. Starts at 1, because `arg(g, 0)` is the program
    // name (`docs/arguments.md` §3).
    at: int,
    // How far into a bundled short run -- `-di` is read at offset 1 and
    // then at offset 2. Zero means "not inside one".
    offset: int,
    // Whether `--` has been seen, after which everything is an operand.
    only_operands: bool,
}

pub fn start() -> [] Cursor {
    return Cursor { at: 1, offset: 0, only_operands: false };
}

// Whether a run of bytes begins with a `-` and is not just `-`.
fn is_flag(a: &static [byte]) -> [] bool {
    return len(a) > 1 && int_of(a[0]) == '-';
}

// The next argument's shape, and where to carry on from.
pub fn step[&g](args: &g Args, c: Cursor) -> [args] (Cursor, Arg) {
    if c.at >= arg_count(args) {
        return (c, Arg::Done);
    }
    let current = arg(args, c.at);

    // Inside a bundled run: `-di` gives `'d'` and then `'i'`.
    if c.offset > 0 {
        let letter = int_of(current[c.offset]);
        var next = Cursor { at: c.at, offset: c.offset + 1, only_operands: false };
        if next.offset >= len(current) {
            next = Cursor { at: c.at + 1, offset: 0, only_operands: false };
        }
        return (next, Arg::Short(letter));
    }

    let after = Cursor { at: c.at + 1, offset: 0, only_operands: c.only_operands };

    if c.only_operands || !is_flag(current) {
        return (after, Arg::Operand(current));
    }

    // `--` on its own ends the flags; `--name` is a long one.
    if int_of(current[1]) == '-' {
        if len(current) == 2 {
            let rest = Cursor { at: c.at + 1, offset: 0, only_operands: true };
            return step(args, rest);
        }
        // The name stops at `=`, and the value after it is `value`'s to
        // find -- a step reports a shape and never a meaning.
        var end = len(current);
        var i = 2;
        while i < end {
            if int_of(current[i]) == '=' {
                end = i;
            }
            i = i + 1;
        }
        return (after, Arg::Long(current[2..end]));
    }

    // A short run. Offset 2 is "one letter left to read", which the
    // branch above picks up; a single `-d` moves straight on.
    var next = Cursor { at: c.at, offset: 2, only_operands: false };
    if len(current) == 2 {
        next = after;
    }
    return (next, Arg::Short(int_of(current[1])));
}

// The value belonging to the flag that was just reported.
//
// The rest of the current argument -- `-d,` and `--delimiter=,` both
// give `,` -- or, when there is none, the whole of the next one, which
// is `-d ,` and `--delimiter ,`. An empty slice means the arguments ran
// out, and that is the one case a caller has to test.
pub fn value[&g](args: &g Args, c: Cursor) -> [args] (Cursor, &static [byte]) {
    // Still inside a bundled run: `-d,` after `-d` leaves `,`.
    if c.offset > 0 && c.at < arg_count(args) {
        let current = arg(args, c.at);
        if c.offset < len(current) {
            let rest = Cursor { at: c.at + 1, offset: 0, only_operands: c.only_operands };
            return (rest, current[c.offset..len(current)]);
        }
    }

    // A long flag's `=value` lives in the argument just passed.
    if c.at > 0 && c.at <= arg_count(args) {
        let previous = arg(args, c.at - 1);
        if len(previous) > 2 && int_of(previous[0]) == '-' && int_of(previous[1]) == '-' {
            var i = 2;
            while i < len(previous) {
                if int_of(previous[i]) == '=' {
                    return (c, previous[i + 1..len(previous)]);
                }
                i = i + 1;
            }
        }
    }

    if c.at < arg_count(args) {
        let after = Cursor { at: c.at + 1, offset: 0, only_operands: c.only_operands };
        return (after, arg(args, c.at));
    }
    return (c, "");
}

// Whether a long flag's name is exactly `name`.
//
// One line, and it exists because the alternative at every call site is
// `bytes.equal(found, "decode")` with the argument order the other way
// round half the time.
pub fn named[&a, &b](found: &a [byte], name: &b [byte]) -> [] bool {
    return bytes.equal(found, name);
}
