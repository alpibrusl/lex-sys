//~ ERROR declared `val`, but it holds
//~ RULE mode-bound-violated

// §3: `val` is a promise about the whole type, so a `res` member breaks it.
// Inferring `res` here instead would make the word decorative.

res struct File { fd: int }

val struct Wrapper {
    f: File,
}

fn main() -> [] int {
    return 0;
}
