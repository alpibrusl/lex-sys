//~ ERROR a reference may not outlive its region

// §6: nothing whose type mentions the arena's region leaves the block. This
// is the same occurs-check as §5's, run by the same code -- which is the
// section's claim, not a coincidence. An arena's lifetime and a borrow's
// lifetime are one mechanism.
//
// `a` is a block, `q` is a region parameter, and no block a function opens
// outlives a region its caller named. So the reference has nowhere to go,
// and the memory it points into is freed one line later.

struct Node {
    value: int,
}

fn escape_arena[&q](fallback: &q Node) -> [] &q Node {
    region a {
        return alloc[a](Node { value: 1 });
    }
}

fn main() -> [] int {
    return 0;
}
