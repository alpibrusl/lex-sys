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
// This file is also the before-and-after for `docs/tuples.md` and
// `docs/shadowing.md`. All three ergonomic gaps `sharing.md` §4 found
// while writing it are now closed: `insert` and `look` hand back tuples
// rather than structs declared for the purpose, `run` is one function
// rather than two, and it threads one `slab` rather than eight names for
// one slab. None of that weakened a rule — `run` still consumes the slab
// at every step, and rebinding it over a live one is still refused.
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

fn show[&i](io: &!i Io, f: Found) -> [io_write] int {
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
fn probe[&i](io: &!i Io, s: Slab, g: Gen, label: &static [byte]) -> [io_write] Slab {
    write_all(io, label);
    let (slab, found) = look(s, g);
    show(io, found);
    putchar(io, 10);
    return slab;
}

// One slab, threaded.
//
// This function is the whole of `sharing.md` §4 answered. It was once
// two functions and eight names for one slab — `fresh`, `filled`,
// `live`, `emptied`, `checked`, `refilled`, `reused`, `stale` — none of
// which was a different thing from the last. Tuples took the two
// declared structs and the split; shadowing took the seven extra names.
//
// The rule that makes it safe is the one that made the old version
// verbose: `slab` may be rebound only because each step *consumed* the
// one before it. Rebinding it over a live slab is still a leak and still
// refused (`docs/shadowing.md` §3.3). The linearity is unchanged; only
// the names are gone.
fn run[&h, &i](heap: &!h Heap, io: &!i Io) -> [heap, io_write] int {
    let slab = new_slab(heap, 4);

    let (slab, first) = insert(slab, 7);
    let slab = probe(io, slab, first, "live handle:  ");

    // Free the slot. One increment of its generation makes every
    // outstanding handle to it stale at once — including this one, which
    // `remove` never saw.
    let slab = remove(slab, first);
    let slab = probe(io, slab, first, "after remove: ");

    // A *new* handle to the same index, with the next generation — so the
    // old handle stays `Missing` while the new one works. That is the
    // whole point of a generation, and the reason a stale handle is safe
    // rather than merely unlikely to be reused.
    //
    // `first` and `second` are two handles alive in one scope, which a
    // struct pattern could not have given: it binds field names, and
    // `insert` used to return one. A tuple pattern names its own
    // bindings (`docs/tuples.md` §1).
    let (slab, second) = insert(slab, 9);
    let slab = probe(io, slab, second, "new handle:   ");
    let slab = probe(io, slab, Gen { index: 0, generation: 0 }, "old handle:   ");
    return drop_slab(heap, slab);
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
