// `slab` — the last unbuilt item on M2's list, and the one that corrected
// the document describing it.
//
// §9 of `linearity-and-effects.md` names two escape hatches for the
// structures linearity cannot express, and says both are "libraries, not
// language features — neither touches the checker". `docs/sharing.md` is
// what building them found: that is true of `Gen`, and **false of `Rc`**.
//
// `Rc` needs N owners of one allocation, which needs a copyable pointer,
// and this language has none — `Box` is linear, a reference is bounded by
// its region, and there is no third thing. Three different ways of trying
// are three fixtures in `tests/reject/`, each refused by a different rule
// that was not written with `Rc` in mind.
//
// This file is also the before-and-after for `docs/tuples.md`. Two of the
// three ergonomic gaps `sharing.md` §4 found while writing it are closed:
// `insert` and `look` hand back tuples rather than structs declared for
// the purpose, and `run` is one function again rather than two. The third
// — no shadowing within a block — is still here, and still visible.
//
// `Gen` works precisely because a handle **points at nothing**: two plain
// `int`s, `val`, copied like any other. The slab owns every value.
//
// What this program shows is the property the whole hatch exists for: a
// handle whose slot was removed comes back `Missing`. Not garbage, not a
// crash, not undefined behaviour — a *value*, which the program decides
// what to do about.
//
// Run it:
//
//     lex-sys build examples/slab/main.ls examples/slab/slab.ls -o slab
//     ./slab

fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io] int {
    var n = 0;
    while n < len(s) {
        putchar(io, int_of(s[n]));
        n = n + 1;
    }
    return len(s);
}

fn print_nat[&i](io: &!i Io, n: int) -> [io] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, 48 + n % 10);
}

fn show[&i](io: &!i Io, f: Found) -> [io] int {
    match f {
        Found::Missing => {
            write_all(io, "missing");
            return 0;
        }
        Found::Value(v) => {
            print_nat(io, v);
            return v;
        }
    }
}

// Look one handle up, say what came back, and hand the slab on.
//
// Several names for one slab below (`fresh`, `filled`, `emptied`,
// `checked`, ...) rather than reassigning one: this language has no
// shadowing within a block. That is the one ergonomic gap `sharing.md`
// §4 recorded that tuples did **not** close, and it is left visible here
// rather than worked around, because a library is a better test of a
// language than a test suite is.
fn probe[&i](io: &!i Io, s: Slab, g: Gen, label: &static [byte]) -> [io] Slab {
    write_all(io, label);
    let (slab, found) = look(s, g);
    show(io, found);
    putchar(io, 10);
    return slab;
}

fn run[&h, &i](heap: &!h Heap, io: &!i Io) -> [heap, io] int {
    let fresh = new_slab(heap, 4);

    let (filled, first) = insert(fresh, 7);
    let live = probe(io, filled, first, "live handle:  ");

    // Free the slot. One increment of its generation makes every
    // outstanding handle to it stale at once — including this one, which
    // `remove` never saw.
    let emptied = remove(live, first);
    let checked = probe(io, emptied, first, "after remove: ");

    // A *new* handle to the same index, with the next generation — so the
    // old handle stays `Missing` while the new one works. That is the
    // whole point of a generation, and the reason a stale handle is safe
    // rather than merely unlikely to be reused.
    //
    // This used to be a second function. `insert` returned a struct that
    // bound `slab` and `handle` by *field name*, and a struct pattern
    // cannot rename, so one scope could not take two of them apart — the
    // gap `sharing.md` §4 recorded second. A tuple pattern names its own
    // bindings (`docs/tuples.md` §1), so `first` and `second` are two
    // handles in one scope and the split is gone.
    let (refilled, second) = insert(checked, 9);
    let reused = probe(io, refilled, second, "new handle:   ");
    let stale = probe(io, reused, Gen { index: 0, generation: 0 }, "old handle:   ");
    return drop_slab(heap, stale);
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);

    var status = 0;
    borrow mut heap as &!h in {
        borrow mut io as &!i in {
            status = run(h, i);
        }
    }
    release(heap);
    release(io);
    return status - 1;
}
