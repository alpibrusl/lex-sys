//~ ERROR declares `telepathy` but never performs it
//~ RULE effect-declared-not-performed

// There is no registry of legal effect labels and there does not need to be.
// A label nothing performs can never appear in an exact row, so §7.3 refuses
// it without anyone having to enumerate what an effect may be called.
//
// Every `io` in every row traces back to a builtin that performs it. A label
// with nothing underneath it is refused the moment it is written.

fn weird() -> [telepathy] int {
    return 1;
}

fn main() -> [] int {
    return 0;
}
