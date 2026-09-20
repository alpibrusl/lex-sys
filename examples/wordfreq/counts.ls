// `wordfreq/counts.ls` — the tally, in a file of its own.
//
// A linked list of (word, count), where a word is a range in the document
// rather than a copy. It needs the heap (`docs/heap.md`) because a list
// that grows as the document is read is exactly the thing a block-scoped
// lifetime cannot hold.

enum Counts {
    Empty,
    Entry(int, int, int, Box[Counts]),
}

// Find a word already counted and increment it **in place**, answering
// whether it was there.
//
// This is what matching a *unique* reference is for
// (`docs/reading-references.md` §2.2). `counts` is `&!c Counts`, so each
// payload binds as `&!c int` -- several unique references at once, sound
// because a variant's payload positions are disjoint -- and `*total =
// *total + 1` writes straight into the node. No rebuilding, no
// reallocation, and the list is not consumed: it is still the caller's
// afterwards.
//
// Read the row: `[]`. This function holds no capability at all, so it
// could not allocate or free if it tried.
fn bump[&c, &t](counts: &!c Counts, text: &t [byte], start: int, length: int) -> [] bool {
    match counts {
        Counts::Empty => { return false; }
        Counts::Entry(at, size, total, rest) => {
            if same_word(text, *at, *size, start, length) {
                *total = *total + 1;
                return true;
            }
            return bump(contents(rest), text, start, length);
        }
    }
}

// A word seen for the first time. This one *does* allocate, and its row
// says `heap`; it consumes the list and hands back a longer one.
fn add[&h](heap: &!h Heap, counts: Counts, start: int, length: int) -> [heap] Counts {
    return Counts::Entry(start, length, 1, box(heap, counts));
}

// Print every word and its count, without spending the list: `&c Counts`
// binds each payload as a shared reference into it.
fn report[&c, &t, &i](counts: &c Counts, text: &t [byte], io: &!i Io) -> [io] int {
    match counts {
        Counts::Empty => { return 0; }
        Counts::Entry(at, size, total, rest) => {
            write_word(io, text, *at, *size);
            putchar(io, 32);
            print_nat(io, *total);
            putchar(io, 10);
            return 1 + report(contents(rest), text, io);
        }
    }
}

// The one path that ends the list, freeing every node exactly once. A
// version of this that forgot a node would not compile (`docs/heap.md`
// §3.1).
fn release_counts[&h](heap: &!h Heap, counts: Counts) -> [heap] int {
    match counts {
        Counts::Empty => { return 0; }
        Counts::Entry(at, size, total, rest) => {
            let tail = unbox(heap, rest);
            return 1 + release_counts(heap, tail);
        }
    }
}
