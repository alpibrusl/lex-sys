// `slab/slab.ls` — §9's `Gen`, as a library.
//
// `docs/sharing.md` §3. A handle is two plain `int`s, so it is `val` and
// copies freely — and it **points at nothing**, which is why copying it is
// safe. The `Slab` owns every value, is `res`, and is ended exactly once
// like any other resource.
//
// The property this exists for: a handle to a removed slot comes back
// `Missing`, which is a **value the program decides what to do about**. A
// dangling pointer is undefined behaviour and the program gets no say.
// §9 calls that asymmetry "most of why this language exists".

val struct Gen {
    index: int,
    generation: int,
}

// One slot. `val`, which is what lets a boxed slice hold a run of them.
struct Entry {
    generation: int,
    live: bool,
    value: int,
}

// One boxed slice, so the values are contiguous: one allocation, one free.
res struct Slab {
    entries: Box[[Entry]],
    live: int,
}

enum Found {
    Missing,
    Value(int),
}

// `insert` has to answer with both a slab and a handle, and this language
// has no tuples (`docs/sharing.md` §4), so every operation that threads
// the slab declares a struct to carry it back.
res struct Inserted {
    slab: Slab,
    handle: Gen,
}

res struct Looked {
    slab: Slab,
    found: Found,
}

fn new_slab[&h](heap: &!h Heap, capacity: int) -> [heap] Slab {
    let empty = Entry { generation: 0, live: false, value: 0 };
    return Slab { entries: box_slice(heap, capacity, empty), live: 0 };
}

fn drop_slab[&h](heap: &!h Heap, s: Slab) -> [heap] int {
    let Slab { entries, live } = s;
    unbox_slice(heap, entries);
    return live;
}

// Fill the first free slot. A scan, not a free list: making it O(1) is a
// policy this library could choose, the way `buffer.ls` chose doubling
// (§3.1). An index of -1 means the slab was full.
fn insert(s: Slab, value: int) -> [] Inserted {
    let Slab { entries, live } = s;
    var at = 0 - 1;
    var generation = 0;
    var added = 0;
    borrow mut entries as &!w in {
        let slots = contents(w);
        var i = 0;
        while i < len(slots) && at < 0 {
            if slots[i].live == false {
                at = i;
                generation = slots[i].generation;
                slots[i] = Entry { generation: generation, live: true, value: value };
                added = 1;
            }
            i = i + 1;
        }
    }
    return Inserted {
        slab: Slab { entries: entries, live: live + added },
        handle: Gen { index: at, generation: generation },
    };
}

// The three checks a handle is worth: in range, live, and the right
// generation. Any of them failing is `Missing`.
fn get[&e](entries: &e Box[[Entry]], g: Gen) -> [] Found {
    let slots = contents(entries);
    if g.index < 0 || g.index >= len(slots) {
        return Found::Missing;
    }
    let slot = slots[g.index];
    if slot.live == false || slot.generation != g.generation {
        return Found::Missing;
    }
    return Found::Value(slot.value);
}

fn look(s: Slab, g: Gen) -> [] Looked {
    let Slab { entries, live } = s;
    var found = Found::Missing;
    borrow entries as &e in {
        found = get(e, g);
    }
    return Looked { slab: Slab { entries: entries, live: live }, found: found };
}

// Free a slot and **bump its generation**, which is the whole mechanism:
// one increment makes every outstanding handle to that slot stale at once,
// including ones this function has never seen.
fn remove(s: Slab, g: Gen) -> [] Slab {
    let Slab { entries, live } = s;
    var removed = 0;
    borrow mut entries as &!w in {
        let slots = contents(w);
        if g.index >= 0 && g.index < len(slots) {
            let slot = slots[g.index];
            if slot.live && slot.generation == g.generation {
                slots[g.index] = Entry {
                    generation: slot.generation + 1,
                    live: false,
                    value: 0,
                };
                removed = 1;
            }
        }
    }
    return Slab { entries: entries, live: live - removed };
}
