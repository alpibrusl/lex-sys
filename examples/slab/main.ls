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
// Four names for one slab below (`fresh`, `filled`, `emptied`, `reused`)
// rather than reassigning one: this language has no shadowing within a
// block, which is one of the three ergonomic gaps §4 of `sharing.md`
// records. A library is a better test of a language than a test suite is.
fn probe[&i](io: &!i Io, s: Slab, g: Gen, label: &static [byte]) -> [io] Slab {
    write_all(io, label);
    let Looked { slab, found } = look(s, g);
    show(io, found);
    putchar(io, 10);
    return slab;
}

fn run[&h, &i](heap: &!h Heap, io: &!i Io) -> [heap, io] int {
    let fresh = new_slab(heap, 4);

    let Inserted { slab, handle } = insert(fresh, 7);
    let filled = probe(io, slab, handle, "live handle:  ");

    // Free the slot. One increment of its generation makes every
    // outstanding handle to it stale at once — including this one, which
    // `remove` never saw.
    let emptied = remove(filled, handle);
    let checked = probe(io, emptied, handle, "after remove: ");

    // Reusing the slot needs a second function, not a second block.
    // `Inserted` binds `slab` and `handle` by field name and this language
    // has no renaming in a pattern, so one scope cannot take two of them
    // apart (`docs/sharing.md` §4). Splitting the function is the honest
    // way round it, and is what a library written in this language ends up
    // doing.
    return reuse(heap, io, checked);
}

// A *new* handle to the same index, with the next generation — so the old
// handle stays `Missing` while the new one works. That is the whole point
// of a generation, and the reason a stale handle is safe rather than
// merely unlikely to be reused.
fn reuse[&h, &i](heap: &!h Heap, io: &!i Io, s: Slab) -> [heap, io] int {
    let Inserted { slab, handle } = insert(s, 9);
    let reused = probe(io, slab, handle, "new handle:   ");
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
