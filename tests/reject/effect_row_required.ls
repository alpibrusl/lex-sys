//~ ERROR expected `[`

// §7.2: *every* function signature declares its row. An absent row would be
// an inferred one, and inference across a boundary is the non-local analysis
// the totality commitment forbids -- so `[]` is written, and purity is
// something a reader can see rather than something a checker worked out.

fn implicit() -> int {
    return 0;
}

fn main() -> [] int {
    return 0;
}
