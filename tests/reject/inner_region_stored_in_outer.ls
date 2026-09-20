//~ ERROR does not outlive

// §6: nesting is the same stack as §5.2's. An inner arena may hold
// references into an outer one -- the outer outlives it -- and never the
// reverse. `i` closes first, so a reference into it stored in a binding that
// belongs to `o` would be dangling for the rest of `o`.
//
// No new rule: `encloses` is a walk up the parent chain, and this is it
// answering the other way round.

struct Node {
    value: int,
}

fn main() -> [] int {
    region o {
        var held = alloc[o](Node { value: 1 });
        region i {
            held = alloc[i](Node { value: 2 });
        }
    }
    return 0;
}
