module fmt.counts;

import fmt.text;

// A module that imports another. `text.` here is this module's binding,
// made by the `import` above -- imports are per *module*, so the other
// file of `fmt.counts`, if there were one, would see the same `text`
// (`docs/modules.md` §4).
//
// `Tally` is `pub` because `main` names it. `step` is not, because
// nothing outside needs it -- and that is the whole of what visibility
// buys: a library can have helpers that are not promises.

pub val struct Tally {
    seen: int,
    total: int,
}

pub fn empty() -> [] Tally {
    return Tally { seen: 0, total: 0 };
}

// Private. A caller in another module asking for this is refused, which
// `tests/` checks; here it is just an implementation detail with a name.
fn step(n: int) -> [] int {
    return n + 1;
}

pub fn add(t: Tally, value: int) -> [] Tally {
    return Tally { seen: step(t.seen), total: t.total + value };
}

pub fn report[&i](io: &!i Io, t: Tally) -> [io_write] int {
    text.write_all(io, "seen ");
    text.print_nat(io, t.seen);
    text.write_all(io, ", total ");
    text.print_nat(io, t.total);
    text.newline(io);
    return t.total;
}
